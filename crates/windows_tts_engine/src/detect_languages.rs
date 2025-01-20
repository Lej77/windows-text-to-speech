use std::{ptr::null_mut, string::FromUtf16Error};

use windows::{
    core::{Error as WinError, GUID},
    Media::SpeechSynthesis::VoiceInformation,
    Win32::Globalization::{
        MappingFreePropertyBag, MappingFreeServices, MappingGetServices, MappingRecognizeText,
        ELS_GUID_LANGUAGE_DETECTION, MAPPING_ENUM_OPTIONS, MAPPING_PROPERTY_BAG,
        MAPPING_SERVICE_INFO,
    },
};

#[derive(Debug)]
pub enum DetectionError {
    MappingGetServices(WinError),
    InvalidServiceGuid,
    MultipleServicesFound,
    MappingRecognizeText(WinError),
    LanguageInvalidUtf16(FromUtf16Error),
    MappingFreePropertyBag(WinError),
}
impl std::fmt::Display for DetectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectionError::MappingGetServices(error) => {
                write!(f, "MappingGetServices failed: {error}")
            }
            DetectionError::InvalidServiceGuid => {
                write!(f, "Incorrect GUID for language detection service")
            }
            DetectionError::MultipleServicesFound => {
                write!(f, "More than one Language Detection service found")
            }
            DetectionError::MappingRecognizeText(error) => {
                write!(f, "MappingRecognizeText failed: {error}")
            }
            DetectionError::LanguageInvalidUtf16(e) => {
                write!(f, "Detected languages codes were not valid UTF-16: {e}")
            }
            DetectionError::MappingFreePropertyBag(e) => {
                write!(f, "MappingFreePropertyBag failed: {e}")
            }
        }
    }
}
impl std::error::Error for DetectionError {}

pub struct DetectedLanguage {
    /// Inclusive start index, the first UTF-16 character this range covers.
    pub start: usize,
    /// Inclusive end index, the last UTF-16 character this range covers.
    pub end: usize,
    /// The identified languages, with the most certain languages earlier in the
    /// list.
    pub languages: Vec<String>,
}
impl DetectedLanguage {
    /// Get the index of a voice's language in the found
    /// [`languages`](Self::languages) list. Lower values are better.
    pub fn get_priority(&self, lang_code: &str) -> Option<usize> {
        self.languages
            .iter()
            .position(|detected| lang_code.starts_with(detected))
    }
}

/// Language detection service info.
pub struct DetectionService {
    service: *mut MAPPING_SERVICE_INFO,
}
impl DetectionService {
    pub fn new() -> Result<Self, DetectionError> {
        // Can use utf16 category but we use GUID directly
        // let mut _category = windows::core::w!("Language Detection");

        // https://learn.microsoft.com/pl-pl/windows/win32/intl/enumerating-and-freeing-services
        let options = MAPPING_ENUM_OPTIONS {
            Size: size_of::<MAPPING_ENUM_OPTIONS>(),
            // pszCategory: PWSTR::from_raw(_category.as_mut_ptr()),
            pGuid: &ELS_GUID_LANGUAGE_DETECTION as *const GUID as *mut GUID,
            ..Default::default() // <- All other fields are zeroed
        };
        let mut services_ptr: *mut MAPPING_SERVICE_INFO = null_mut();
        let mut len = 0;
        unsafe { MappingGetServices(Some(&options), &mut services_ptr, &mut len) }
            .map_err(DetectionError::MappingGetServices)?;

        // This object will call `MappingFreeServices` later:
        let service = DetectionService {
            service: services_ptr,
        };
        let services = unsafe { std::slice::from_raw_parts(services_ptr, len as usize) };
        let first = services[0];
        if first.guid != ELS_GUID_LANGUAGE_DETECTION {
            return Err(DetectionError::InvalidServiceGuid);
        }
        if len != 1 {
            return Err(DetectionError::MultipleServicesFound);
        }
        Ok(service)
    }

    pub fn recognize_text(
        &self,
        text_utf16: &[u16],
    ) -> Result<Vec<DetectedLanguage>, DetectionError> {
        let mut prop_bag = MAPPING_PROPERTY_BAG {
            Size: size_of::<MAPPING_PROPERTY_BAG>(),
            ..Default::default()
        };
        unsafe {
            MappingRecognizeText(
                // Note: can't have called MappingFreeServices before this point
                self.service,
                // text without trailing nuls:
                text_utf16.strip_suffix(&[0]).unwrap_or(text_utf16),
                0,
                None,
                &mut prop_bag,
            )
        }
        .map_err(DetectionError::MappingRecognizeText)?;

        let mut detected = Vec::new();

        let result_ranges = unsafe {
            std::slice::from_raw_parts(prop_bag.prgResultRanges, prop_bag.dwRangesCount as usize)
        };
        for range in result_ranges {
            let data = unsafe {
                std::slice::from_raw_parts(range.pData as *const u16, range.dwDataSize as usize / 2)
            };
            let languages = data
                .strip_suffix(&[0])
                .expect("there should be trailing nul characters") // two trailing nul characters
                .split(|&v| v == 0) // then one nul between every two detected langs
                .map(String::from_utf16) // text is utf16 encoded
                .collect::<Result<Vec<String>, _>>()
                .map_err(DetectionError::LanguageInvalidUtf16)?;

            detected.push(DetectedLanguage {
                start: range.dwStartIndex as usize,
                end: range.dwEndIndex as usize,
                languages,
            })
        }

        unsafe { MappingFreePropertyBag(&prop_bag) }
            .map_err(DetectionError::MappingFreePropertyBag)?;
        Ok(detected)
    }
}
impl Drop for DetectionService {
    fn drop(&mut self) {
        // TODO: log error
        _ = unsafe { MappingFreeServices(self.service) };
    }
}
