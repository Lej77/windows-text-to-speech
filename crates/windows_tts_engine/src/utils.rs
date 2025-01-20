use std::{
    any::Any,
    ffi::OsStr,
    panic::{catch_unwind, AssertUnwindSafe, UnwindSafe},
};

use windows::Win32::{
    Foundation::{HMODULE, MAX_PATH},
    System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    },
};
use windows_core::{GUID, PCWSTR};

/// Ensures that dropping the provided value doesn't panic.
pub fn safe_drop<T>(value: T) {
    if let Err(e) = catch_unwind(AssertUnwindSafe(|| drop(value))) {
        safe_any_drop(e);
    }
}

/// Ensures that a panic payload doesn't panic when dropped.
pub fn safe_any_drop(mut value: Box<dyn Any>) {
    loop {
        match catch_unwind(AssertUnwindSafe(|| drop(value))) {
            Ok(()) => break,
            Err(e) => value = e,
        }
    }
}

/// Safe as [`catch_unwind`] except it ensures that caught panic payloads don't
/// panic.
pub fn safe_catch_unwind<F: FnOnce() -> R + UnwindSafe, R>(f: F) -> Option<R> {
    catch_unwind(f).map_err(|e| safe_any_drop(e)).ok()
}

/// Catch unwinds and turn them into errors.
pub(crate) fn catch_unwind_and_fail<R>(
    f: impl FnOnce() -> windows_core::Result<R>,
) -> windows_core::Result<R> {
    safe_catch_unwind(AssertUnwindSafe(f))
        .ok_or_else(|| windows_core::Error::from(windows::Win32::Foundation::E_FAIL))
        .unwrap_or_else(|e| Err(e))
}

/// UTF-16 encode something that can be represented as a Windows string, for
/// example [`str`] or [`PathBuf`](std::path::PathBuf).
pub fn to_utf16<T: AsRef<OsStr>>(s: T) -> Vec<u16> {
    fn inner(s: &OsStr) -> Vec<u16> {
        std::os::windows::ffi::OsStrExt::encode_wide(s)
            .chain(core::iter::once(0u16))
            .collect()
    }
    inner(s.as_ref())
}

/// Same as the current [`core::fmt::Debug`] formatting of [`GUID`], but uses
/// the [`core::fmt::Display`] trait. Debug formatting is generally not
/// guaranteed to stay the same when upgrading a libraries version.
pub fn display_guid(guid: GUID) -> impl core::fmt::Display {
    struct DisplayGuid(GUID);
    impl core::fmt::Display for DisplayGuid {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            // Copied from `<windows_core::guid::Guid as Debug>::fmt` method (we
            // don't want to rely on Debug formatting):
            write!(
                f,
                "{:08X?}-{:04X?}-{:04X?}-{:02X?}{:02X?}-{:02X?}{:02X?}{:02X?}{:02X?}{:02X?}{:02X?}",
                self.0.data1,
                self.0.data2,
                self.0.data3,
                self.0.data4[0],
                self.0.data4[1],
                self.0.data4[2],
                self.0.data4[3],
                self.0.data4[4],
                self.0.data4[5],
                self.0.data4[6],
                self.0.data4[7]
            )
        }
    }
    DisplayGuid(guid)
}

/// Get handle for this code's module.
///
/// Note: this doesn't increment the module reference count and so the returned
/// module should not be freed using [`windows::core::Free::free`].
///
/// Adapted from:
/// <https://stackoverflow.com/questions/557081/how-do-i-get-the-hmodule-for-the-currently-executing-code/557774#557774>
pub fn get_current_module() -> windows::core::Result<HMODULE> {
    let mut module = HMODULE::default();
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR::from_raw(get_current_module as fn() -> _ as *const _),
            &mut module,
        )
    }?;

    Ok(module)
}

/// If this code is included inside a DLL file then this will get the path to
/// that file.
pub fn get_current_dll_path(
    buffer: &mut [u16; MAX_PATH as usize],
) -> windows::core::Result<&mut [u16]> {
    let module = get_current_module()?;
    let len = unsafe { GetModuleFileNameW(Some(module), buffer) };
    if len == 0 || len == MAX_PATH {
        Err(windows::core::Error::from_win32())
    } else {
        Ok(&mut buffer[..len as usize + 1])
    }
}
