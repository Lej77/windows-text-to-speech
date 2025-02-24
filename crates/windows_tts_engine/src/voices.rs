//! Register text-to-speech voices/engines with Windows.

use crate::utils::{display_guid, to_utf16};
use windows::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, E_FAIL},
    System::Registry::{
        RegCreateKeyExW, RegDeleteKeyExW, RegSetValueExW, HKEY, KEY_SET_VALUE, REG_SZ,
    },
};
use windows_core::{w, Free, GUID, PCWSTR};

#[derive(Debug, Clone, Copy)]
pub enum ParentRegKey<'a> {
    Path(HKEY, &'a str),
    Handle(HKEY),
}
impl ParentRegKey<'_> {
    pub fn parent_handle(self) -> HKEY {
        match self {
            ParentRegKey::Path(hkey, _) => hkey,
            ParentRegKey::Handle(hkey) => hkey,
        }
    }
    fn ending_separator(prefix: &str) -> &'static str {
        if prefix.ends_with(['\\', '/']) {
            ""
        } else {
            "\\"
        }
    }
    pub fn sub_key_path(self, sub_key: &str, buffer: &mut Vec<u16>) -> PCWSTR {
        if let ParentRegKey::Path(_, prefix) = self {
            let separator = Self::ending_separator(prefix);
            *buffer = to_utf16(format!("{prefix}{separator}{sub_key}"));
            ::windows::core::PCWSTR::from_raw(buffer.as_ptr())
        } else {
            *buffer = to_utf16(sub_key);
            ::windows::core::PCWSTR::from_raw(buffer.as_ptr())
        }
    }
    pub fn join_sub_key<'b>(self, sub_key: &'b str, buffer: &'b mut String) -> ParentRegKey<'b> {
        match self {
            ParentRegKey::Path(hkey, prefix) => {
                *buffer = format!("{prefix}{}{sub_key}", Self::ending_separator(prefix));
                ParentRegKey::Path(hkey, buffer.as_str())
            }
            ParentRegKey::Handle(hkey) => ParentRegKey::Path(hkey, sub_key),
        }
    }
}

/// Voice metadata stored in Windows registry. See [`VoiceKeyData`] for more
/// info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceAttributes {
    /// Example: "Microsoft David" or "eSpeak-en"
    pub name: String,
    /// Example: "Female" or "Male"
    pub gender: String,
    /// Example: "Adult"
    pub age: String,
    /// Example: "409" or "809"
    pub language: String,
    /// Example: "Microsoft" or "http://espeak.sf.net"
    pub vendor: String,
}
impl VoiceAttributes {
    pub fn write_to_registry(&self, voice_key: ParentRegKey) -> windows::core::Result<()> {
        let mut attributes_key = Default::default();

        let mut sub_key_buffer = Vec::new();
        unsafe {
            RegCreateKeyExW(
                voice_key.parent_handle(),
                voice_key.sub_key_path("Attributes", &mut sub_key_buffer),
                None,
                None,
                Default::default(),
                KEY_SET_VALUE,
                None,
                &mut attributes_key,
                None,
            )
        }
        .ok()?;

        let values_to_set = [
            ("Name", self.name.as_str()),
            ("Gender", self.gender.as_str()),
            ("Age", self.age.as_str()),
            ("Language", self.language.as_str()),
            ("Vendor", self.vendor.as_str()),
        ];

        for (name, value) in values_to_set {
            let name = to_utf16(name);
            let value = to_utf16(value);
            unsafe {
                RegSetValueExW(
                    attributes_key,
                    PCWSTR::from_raw(name.as_ptr()),
                    None,
                    REG_SZ,
                    Some(value.align_to().1),
                )
            }
            .ok()?;
        }

        unsafe { attributes_key.free() };

        Ok(())
    }
    pub fn remove_from_registry(&self, voice_key: ParentRegKey) -> windows::core::Result<()> {
        let mut sub_key_buffer = Vec::new();
        let result = unsafe {
            RegDeleteKeyExW(
                voice_key.parent_handle(),
                voice_key.sub_key_path("Attributes", &mut sub_key_buffer),
                0,
                None,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            result.ok()
        }
    }
}

/// Registry data associated with a text-to-speech voice.
///
/// # References
///
/// - [`VoiceTokenEnumerator::MakeLocalVoiceToken` in the GitHub project `gexgd0419/NaturalVoiceSAPIAdapter`](https://github.com/gexgd0419/NaturalVoiceSAPIAdapter/blob/2573a979a71ee96d3370676dd6f6acb382e4d35e/NaturalVoiceSAPIAdapter/VoiceTokenEnumerator.cpp#L298-L326)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceKeyData {
    /// The name of the registry key. Can be `Voice1` or anything else. Should
    /// not contain path separators.
    ///
    /// Example: "TTS_MS_EN-US_DAVID_11.0", "MSTTS_V110_enUS_DavidM" or "eSpeak_1"
    pub key_name: String,
    /// Stored in the key's default value.
    ///
    /// Example: "Microsoft David - English (United States)" or "eSpeak-EN"
    pub long_name: String,
    /// Get COM class id for the text-to-speech engine that will handle this
    /// voice. The voice token will be given to the engine's
    /// [`SafeTtsEngine::set_object_token`] method.
    pub class_id: GUID,
    pub attributes: VoiceAttributes,
}
impl VoiceKeyData {
    /// Create a registry key with data about a voice inside a `Tokens` folder
    /// specified by a key handle.
    pub fn write_to_registry(&self, tokens_key: ParentRegKey) -> windows::core::Result<()> {
        if self.key_name.contains(['/', '\\']) {
            return Err(windows::core::Error::new(
                E_FAIL,
                "Registry keys can not contain path separators",
            ));
        }

        let mut key = Default::default();
        {
            let mut key_name_buffer = Vec::new();
            unsafe {
                RegCreateKeyExW(
                    tokens_key.parent_handle(),
                    tokens_key.sub_key_path(&self.key_name, &mut key_name_buffer),
                    None,
                    None,
                    Default::default(),
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
            }
            .ok()?;
        }

        {
            let long_name = to_utf16(&self.long_name);
            unsafe {
                RegSetValueExW(
                    key,
                    PCWSTR::null(),
                    None,
                    REG_SZ,
                    Some(long_name.align_to().1),
                )
            }
            .ok()?;
        }

        {
            let bracketed_class_id = to_utf16(format!("{{{}}}", display_guid(self.class_id)));
            unsafe {
                RegSetValueExW(
                    key,
                    w!("CLSID"),
                    None,
                    REG_SZ,
                    Some(bracketed_class_id.align_to().1),
                )
            }
            .ok()?;
        }

        self.attributes
            .write_to_registry(ParentRegKey::Handle(key))?;

        unsafe { key.free() };
        Ok(())
    }
    pub fn remove_from_registry(&self, tokens_key: ParentRegKey) -> windows::core::Result<()> {
        {
            let mut buffer = String::new();
            self.attributes
                .remove_from_registry(tokens_key.join_sub_key(&self.key_name, &mut buffer))?;
        }

        let mut voice_key = Vec::new();
        let result = unsafe {
            RegDeleteKeyExW(
                tokens_key.parent_handle(),
                tokens_key.sub_key_path(&self.key_name, &mut voice_key),
                0,
                None,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            result.ok()
        }
    }
}

mod private_impls {
    //! Inner module to make the generated [`VoiceTokenEnumerator_Impl`] type
    //! private since its trait implementation has methods that should be unsafe
    //! to call.

    use windows::Win32::Media::Speech::{
        IEnumSpObjectTokens, IEnumSpObjectTokens_Impl, ISpObjectToken,
    };
    use windows_core::implement;

    /// An iterator that lists text-to-speech voices.
    ///
    /// # References
    ///
    /// - [Reimplement the SAPI bindings. · Issue #7 · espeak-ng/espeak-ng](https://github.com/espeak-ng/espeak-ng/issues/7#issuecomment-2527109323)
    #[implement(IEnumSpObjectTokens)]
    pub struct VoiceTokenEnumerator(());

    impl IEnumSpObjectTokens_Impl for VoiceTokenEnumerator_Impl {
        fn Next(
            &self,
            _celt: u32,
            _pelt: windows_core::OutRef<'_, ISpObjectToken>,
            _pceltfetched: *mut u32,
        ) -> windows_core::Result<()> {
            todo!()
        }

        fn Skip(&self, _celt: u32) -> windows_core::Result<()> {
            todo!()
        }

        fn Reset(&self) -> windows_core::Result<()> {
            todo!()
        }

        fn Clone(&self) -> windows_core::Result<IEnumSpObjectTokens> {
            todo!()
        }

        fn Item(&self, _index: u32) -> windows_core::Result<ISpObjectToken> {
            todo!()
        }

        fn GetCount(&self, _pcount: *mut u32) -> windows_core::Result<()> {
            todo!()
        }
    }
}

pub use private_impls::VoiceTokenEnumerator;
