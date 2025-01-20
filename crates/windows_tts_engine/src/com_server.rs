//! Define and register a COM Server.
//!
//! # References
//!
//! - [Registering COM Servers - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/com/registering-com-servers)
//! - Relevant for Rust code
//!   - COM Server in Rust:\
//!     [rust - Implementing a Windows Credential Provider - Stack Overflow](https://stackoverflow.com/questions/75279682/implementing-a-windows-credential-provider)
//!   - ["Module not found" error when using the Rust COM class · Issue #142 · microsoft/com-rs](https://github.com/microsoft/com-rs/issues/142)
//!   - Note `stcall` and `system` ABI is the same on Windows, see:\
//!     [External blocks - The Rust Reference](https://doc.rust-lang.org/reference/items/external-blocks.html)

use crate::{
    utils::{display_guid, get_current_dll_path, safe_catch_unwind, to_utf16},
    SafeTtsEngine,
};
use std::{
    borrow::Cow,
    path::Path,
    ptr,
    sync::{Arc, OnceLock},
};

use windows::Win32::{
    Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, ERROR_FILE_NOT_FOUND, E_INVALIDARG, E_POINTER, E_UNEXPECTED,
        MAX_PATH, S_FALSE, S_OK,
    },
    System::{
        Com::IClassFactory,
        Ole::SELFREG_E_CLASS,
        Registry::{
            RegCreateKeyExW, RegDeleteKeyExW, RegSetValueExW, HKEY_CLASSES_ROOT, KEY_SET_VALUE,
            KEY_WOW64_64KEY, REG_SZ,
        },
    },
};
use windows_core::{w, Error as WinError, Free, Interface, GUID, PCWSTR};

/// Every COM class created needs to contain an owned `Arc` cloned from this
/// value in order to prevent unloading this COM Module while classes from it
/// are still alive.
pub fn module_ref() -> &'static Arc<()> {
    static MODULE: OnceLock<Arc<()>> = OnceLock::new();
    MODULE.get_or_init(|| Arc::new(()))
}

fn safe_init_once<T: SafeTtsComServer>() {
    static SAFE_ONCE_INIT: std::sync::Once = std::sync::Once::new();
    SAFE_ONCE_INIT.call_once(|| {
        safe_catch_unwind::<_, ()>(|| T::initialize());
    });
}

/// A safe alternative to the [`ComServer`] trait.
pub trait SafeTtsComServer: ComServer {
    /// Class id of the text-to-speech engine.
    const CLSID_TTS_ENGINE: GUID;

    /// The type of the text-to-speech engine.
    type TtsEngine: SafeTtsEngine;

    /// Create a text-to-speech engine.
    fn create_engine() -> Self::TtsEngine;

    /// Register the COM Class id
    /// [`CLSID_TTS_ENGINE`](SafeTtsComServer::CLSID_TTS_ENGINE) with Windows
    /// using [`ComClassInfo::register`]. Also register the text-to-speech
    /// voice/engine with Windows using
    /// [`voices::VoiceKeyData`](crate::voices::VoiceKeyData).
    fn register_server();

    /// Undo the actions made by
    /// [`register_server`](SafeTtsComServer::register_server).
    fn unregister_server();

    /// Called once. Can be used to for example setup logging.
    fn initialize() {}
}
unsafe impl<T> ComServer for T
where
    T: SafeTtsComServer,
{
    unsafe fn DllGetClassObject(
        rclsid: *const windows::core::GUID,
        riid: *const windows::core::GUID,
        ppv: *mut *mut ::core::ffi::c_void,
    ) -> windows::core::HRESULT {
        safe_catch_unwind(|| {
            safe_init_once::<Self>();
            log::debug!("DllGetClassObject");

            // Validate arguments
            if ppv.is_null() {
                return E_POINTER;
            }
            unsafe { ppv.write(ptr::null_mut()) };
            if rclsid.is_null() || riid.is_null() {
                return E_INVALIDARG;
            }

            let rclsid = unsafe { rclsid.read() };
            let riid = unsafe { riid.read() };
            // The following isn't strictly correct; a client *could* request an interface other
            // than `IClassFactory::IID`, which this implementation is simply failing.
            // This is safe, even if overly restrictive
            if rclsid != Self::CLSID_TTS_ENGINE || riid != IClassFactory::IID {
                return CLASS_E_CLASSNOTAVAILABLE;
            }

            // Construct the factory object and return its `IClassFactory` interface
            let com_module: Arc<()> = module_ref().clone();

            // Note: the `WindowsTtsEngineFactory` COM class will contain
            //       `com_module` and drop it when the COM class is released.
            let factory = IClassFactory::from(crate::WindowsTtsEngineFactory::new(
                Self::CLSID_TTS_ENGINE,
                Some(com_module.clone()),
                move || {
                    log::debug!("Factory created new text-to-speech engine");
                    Self::create_engine()
                },
            ));
            unsafe { ppv.write(factory.into_raw()) };
            log::debug!("DllGetClassObject -> Ok");
            S_OK
        })
        .unwrap_or(E_UNEXPECTED)
    }

    fn DllCanUnloadNow() -> windows::core::HRESULT {
        safe_catch_unwind(|| {
            safe_init_once::<Self>();
            // Since we aren't tracking module references (yet), it's never safe to unload this
            // module
            if Arc::strong_count(module_ref()) == 1 {
                // It is safe to unload this module
                log::debug!("DllCanUnloadNow -> true");
                S_OK
            } else {
                // Since we are tracking alive module references in created COM classes
                // it is not safe to unload this module
                log::debug!("DllCanUnloadNow -> false");
                S_FALSE
            }
        })
        .unwrap_or(S_FALSE)
    }

    fn DllRegisterServer() -> windows::core::HRESULT {
        safe_catch_unwind(|| {
            safe_init_once::<Self>();
            log::debug!("DllRegisterServer");
            Self::register_server();
            S_OK
        })
        .unwrap_or(SELFREG_E_CLASS)
    }

    fn DllUnregisterServer() -> windows::core::HRESULT {
        safe_catch_unwind(|| {
            safe_init_once::<Self>();
            log::debug!("DllUnregisterServer");
            Self::unregister_server();
            S_OK
        })
        .unwrap_or(SELFREG_E_CLASS)
    }
}

/// Entry points for a DLL COM Server.
///
/// Export the functions from a DLL using [`dll_com_server_fns`]
///
/// # Safety
///
/// - The trait functions are not allowed to unwind.
/// - The trait functions must be correctly implemented.
///   - Must write to out pointers.
///   - [`ComServer::DllCanUnloadNow`] can only return
///     [`S_OK`](windows::Win32::Foundation::S_OK) when all created COM classes
///     have been destroyed.
#[expect(non_snake_case, reason = "match windows API names")]
pub unsafe trait ComServer: Send + Sync + 'static {
    /// # Safety
    ///
    /// - The `*const` pointers are null or valid to read from.
    /// - The `*mut` pointer is null or valid to write through.
    unsafe fn DllGetClassObject(
        rclsid: *const windows::core::GUID,
        riid: *const windows::core::GUID,
        ppv: *mut *mut ::core::ffi::c_void,
    ) -> windows::core::HRESULT;

    /// Return [`S_OK`](windows::Win32::Foundation::S_OK) if the module can be
    /// unloaded and [`S_FALSE`](windows::Win32::Foundation::S_FALSE) if it
    /// can't.
    ///
    /// The module can only be unloaded if all COM classes that has been created
    /// by it has also been closed. Use a global reference count to track this.
    fn DllCanUnloadNow() -> windows::core::HRESULT;

    /// Use `regsvr32.exe` with the DLL path to invoke this.
    fn DllRegisterServer() -> windows::core::HRESULT;
    /// Use `regsvr32.exe` with the DLL path and the `/u` flag to invoke this.
    fn DllUnregisterServer() -> windows::core::HRESULT;
}

/// Provide with a type that implements [`ComServer`]. Generates `no_mangle`
/// functions for each of the trait's associated functions.
#[doc(hidden)] //  <- hide from crate root docs
#[macro_export] // <- exported from crate root, so we later use re-export to make it visible from this module path
macro_rules! _dll_export_com_server_fns {
    ($server:ty) => {
        /// # References
        ///
        /// Signature from: [rust - Implementing a Windows Credential Provider - Stack Overflow](https://stackoverflow.com/questions/75279682/implementing-a-windows-credential-provider)
        #[no_mangle]
        pub unsafe extern "stdcall" fn DllGetClassObject(
            rclsid: *const $crate::windows::core::GUID,
            riid: *const $crate::windows::core::GUID,
            ppv: *mut *mut ::core::ffi::c_void,
        ) -> $crate::windows::core::HRESULT {
            <$server as $crate::com_server::ComServer>::DllGetClassObject(rclsid, riid, ppv)
        }

        /// # References
        ///
        /// Signature from: [rust - Implementing a Windows Credential Provider - Stack Overflow](https://stackoverflow.com/questions/75279682/implementing-a-windows-credential-provider)
        #[no_mangle]
        pub unsafe extern "stdcall" fn DllCanUnloadNow() -> $crate::windows::core::HRESULT {
            <$server as $crate::com_server::ComServer>::DllCanUnloadNow()
        }

        #[no_mangle]
        pub extern "stdcall" fn DllRegisterServer() -> $crate::windows::core::HRESULT {
            <$server as $crate::com_server::ComServer>::DllRegisterServer()
        }

        #[no_mangle]
        pub extern "stdcall" fn DllUnregisterServer() -> $crate::windows::core::HRESULT {
            <$server as $crate::com_server::ComServer>::DllUnregisterServer()
        }
    };
}
pub use _dll_export_com_server_fns as dll_export_com_server_fns;

/// Specifies the threading model of the apartment the server can run in. See
/// [InprocServer32 - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/com/inprocserver32).
#[derive(Debug, Clone, Copy)]
pub enum ComThreadingModel {
    /// Single-threaded apartment.
    Apartment,
    /// Single-threaded or multithreaded apartment.
    Both,
    /// Multithreaded apartment.
    Free,
    /// Neutral apartment.
    Neutral,
}

/// Path to COM Server.
#[derive(Debug, Clone)]
pub enum ComServerPath<'a> {
    /// Path of the DLL or EXE that the current code is inside.
    CurrentModule,
    /// Rust uses an encoding that is a superset UTF-8 and allows all valid
    /// Windows paths.
    RustPath(Cow<'a, Path>),
    /// UTF-16 encoded path without trailing nul character.
    Utf16Path(Cow<'a, [u16]>),
}
impl ComServerPath<'_> {
    pub fn into_owned(self) -> ComServerPath<'static> {
        match self {
            ComServerPath::CurrentModule => ComServerPath::CurrentModule,
            ComServerPath::RustPath(cow) => ComServerPath::RustPath(cow.into_owned().into()),
            ComServerPath::Utf16Path(cow) => ComServerPath::Utf16Path(cow.into_owned().into()),
        }
    }
    pub fn to_utf16_path<'buf>(
        &'buf self,
        buffer: &'buf mut [u16; MAX_PATH as usize],
    ) -> windows::core::Result<&'buf [u16]> {
        Ok(match self {
            ComServerPath::CurrentModule => get_current_dll_path(buffer)?,
            ComServerPath::RustPath(cow) => {
                let utf_16 = to_utf16(&**cow);
                let buffer = &mut buffer[..utf_16.len()];
                buffer.copy_from_slice(&utf_16);
                buffer
            }
            ComServerPath::Utf16Path(cow) => &**cow,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ComClassRegisterError {
    CreateRegisterKey(WinError),
    ComClassName(WinError),
    CreateInprocServer32(WinError),
    GetCurrentModelPath(WinError),
    InprocServer32Path(WinError),
    ThreadingModel(WinError),
}
impl std::fmt::Display for ComClassRegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComClassRegisterError::CreateRegisterKey(error) => {
                write!(f, "Failed to create registry key for COM Server: {error}")
            }
            ComClassRegisterError::ComClassName(error) => write!(
                f,
                "Failed to store descriptive name as default \
                value for COM Server registry key: {error}"
            ),
            ComClassRegisterError::CreateInprocServer32(error) => write!(
                f,
                "Failed to create \"InprocServer32\" \
                registry sub key for COM Server: {error}"
            ),
            ComClassRegisterError::GetCurrentModelPath(error) => {
                write!(
                    f,
                    "Failed to get path for dll that should be registered: {error}"
                )
            }
            ComClassRegisterError::InprocServer32Path(error) => write!(
                f,
                "Failed to store dll/exe path as default value for \
                COM Server \"InprocServer32\" registry sub key: {error}"
            ),
            ComClassRegisterError::ThreadingModel(error) => write!(
                f,
                "Failed to set ThreadingModel key for COM Server registry sub key: {error}"
            ),
        }
    }
}
impl std::error::Error for ComClassRegisterError {}

/// Info required to register a COM Class.
#[derive(Debug, Clone)]
pub struct ComClassInfo<'a> {
    /// Id of the COM Class.
    pub clsid: GUID,
    /// Optional descriptive name of the COM Class.
    pub class_name: Option<Cow<'a, str>>,
    /// Threading model for the COM Server that owns the COM Class.
    pub threading_model: ComThreadingModel,
    /// Absolute file path to the DLL or EXE that can create the COM Class.
    pub server_path: ComServerPath<'a>,
}
impl ComClassInfo<'_> {
    pub fn into_owned(self) -> ComClassInfo<'static> {
        ComClassInfo {
            clsid: self.clsid,
            class_name: self.class_name.map(|name| Cow::Owned(name.into_owned())),
            threading_model: self.threading_model,
            server_path: self.server_path.into_owned(),
        }
    }
    pub fn register(&self) -> Result<(), ComClassRegisterError> {
        let class_path = to_utf16(format!("CLSID\\{{{}}}", display_guid(self.clsid)));

        let mut key = Default::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CLASSES_ROOT,
                PCWSTR::from_raw(class_path.as_ptr()),
                None,
                None,
                Default::default(),
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .map_err(ComClassRegisterError::CreateRegisterKey)?;

        if let Some(class_name) = &self.class_name {
            let class_name = to_utf16(&**class_name);
            unsafe {
                RegSetValueExW(
                    key,
                    PCWSTR::null(),
                    None,
                    REG_SZ,
                    Some(class_name.align_to().1),
                )
            }
            .ok()
            .map_err(ComClassRegisterError::ComClassName)?;
        }

        let mut sub_key = Default::default();
        unsafe {
            RegCreateKeyExW(
                key,
                w!("InprocServer32"),
                None,
                None,
                Default::default(),
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &mut sub_key,
                None,
            )
        }
        .ok()
        .map_err(ComClassRegisterError::CreateInprocServer32)?;

        // Dll path in default value:
        {
            let mut buf = [0; MAX_PATH as _];
            let dll_path = self
                .server_path
                .to_utf16_path(&mut buf)
                .map_err(ComClassRegisterError::GetCurrentModelPath)?;

            unsafe {
                RegSetValueExW(
                    sub_key,
                    PCWSTR::null(),
                    None,
                    REG_SZ,
                    Some(dll_path.align_to().1),
                )
            }
            .ok()
            .map_err(ComClassRegisterError::InprocServer32Path)?;
        }

        // ThreadingModel:
        {
            // https://learn.microsoft.com/en-us/windows/win32/com/inprocserver32
            let threading_model = match self.threading_model {
                ComThreadingModel::Apartment => w!("Apartment"),
                ComThreadingModel::Both => w!("Both"),
                ComThreadingModel::Free => w!("Free"),
                ComThreadingModel::Neutral => w!("Neutral"),
            };
            unsafe {
                RegSetValueExW(
                    sub_key,
                    w!("ThreadingModel"),
                    None,
                    REG_SZ,
                    Some(threading_model.as_wide().align_to().1),
                )
            }
            .ok()
            .map_err(ComClassRegisterError::ThreadingModel)?;
        }

        unsafe {
            key.free();
            sub_key.free();
        }
        Ok(())
    }
    pub fn unregister_class_id(clsid: GUID) -> windows::core::Result<()> {
        let class_sub_key_path = to_utf16(format!(
            "CLSID\\{{{}}}\\InprocServer32",
            display_guid(clsid)
        ));
        let class_key_path = to_utf16(format!("CLSID\\{{{}}}", display_guid(clsid)));

        // Note: order matters since sub keys must be deleted first.
        let keys_to_delete = [
            PCWSTR::from_raw(class_sub_key_path.as_ptr()),
            PCWSTR::from_raw(class_key_path.as_ptr()),
        ];

        for key_to_delete in keys_to_delete {
            let result = unsafe {
                RegDeleteKeyExW(HKEY_CLASSES_ROOT, key_to_delete, KEY_WOW64_64KEY.0, None)
            };
            if result != ERROR_FILE_NOT_FOUND {
                result.ok()?;
            }
        }
        Ok(())
    }
}
