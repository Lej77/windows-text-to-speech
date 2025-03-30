#[cfg(feature = "lingua")]
use std::str::FromStr;
use std::{ptr::null_mut, string::FromUtf16Error};

use windows::{
    core::{Error as WinError, GUID},
    Win32::Globalization::{
        MappingFreePropertyBag, MappingFreeServices, MappingGetServices, MappingRecognizeText,
        ELS_GUID_LANGUAGE_DETECTION, MAPPING_ENUM_OPTIONS, MAPPING_PROPERTY_BAG,
        MAPPING_SERVICE_INFO,
    },
};

#[cfg(feature = "lingua")]
use lingua::{IsoCode639_1, IsoCode639_3, Language, LanguageDetector, LanguageDetectorBuilder};

pub fn equal_language_codes(first: &str, second: &str) -> bool {
    const SEPARATORS: [char; 2] = ['_', '-'];

    if first.contains(SEPARATORS) && second.contains(SEPARATORS) {
        // Only care about suffixes like `US` if both codes contain them `en-US`.
        first == second
    } else {
        first
            .split_once(SEPARATORS)
            .map(|(prefix, _)| prefix)
            .unwrap_or(first)
            == second
                .split_once(SEPARATORS)
                .map(|(prefix, _)| prefix)
                .unwrap_or(second)
    }
}

pub fn has_multiple_languages<S>(languages: impl IntoIterator<Item = S>) -> bool
where
    S: AsRef<str>,
{
    let mut languages = languages.into_iter();
    let Some(first) = languages.next() else {
        return false;
    };
    // Search for another language:
    languages.any(|other| !equal_language_codes(first.as_ref(), other.as_ref()))
}

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
            .position(|detected| equal_language_codes(detected, lang_code))
    }
}

/// Language detection service handle for Microsoft Language Detection.
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

enum LinguaDetectionServiceState {
    #[cfg(feature = "lingua")]
    Lingua(Box<LanguageDetector>),
    Microsoft(DetectionService),
}

/// Language detection using the [`lingua`] crate or using the Microsoft
/// Language Detection ([`DetectionService`]).
pub struct LinguaDetectionService {
    state: LinguaDetectionServiceState,
}
impl LinguaDetectionService {
    /// Use [`lingua`] for language detection if the `lingua` Cargo feature is enabled, otherwise use
    /// [`DetectionService`] for language detection.
    pub fn with_lingua<S: AsRef<str>>(_languages: &[S]) -> Result<Self, DetectionError> {
        #[cfg(feature = "lingua")]
        {
            let languages: Vec<Language> = _languages
                .iter()
                .map(AsRef::as_ref)
                // ignore suffix in codes like "en-US"
                .map(|lang| {
                    lang.split_once(['_', '-'])
                        .map(|(prefix, _)| prefix)
                        .unwrap_or(lang)
                })
                .filter_map(|lang| match IsoCode639_1::from_str(lang) {
                    Ok(v) => Some(Language::from_iso_code_639_1(&v)),
                    Err(_) => match IsoCode639_3::from_str(lang) {
                        Ok(v) => Some(Language::from_iso_code_639_3(&v)),
                        Err(_) => {
                            log::warn!("Failed to identify language {lang:?}");
                            None
                        }
                    },
                })
                .collect();
            Ok(Self {
                state: LinguaDetectionServiceState::Lingua(Box::new(
                    LanguageDetectorBuilder::from_languages(&languages).build(),
                )),
            })
        }

        #[cfg(not(feature = "lingua"))]
        Self::with_microsoft_language_detection()
    }
    pub fn with_microsoft_language_detection() -> Result<Self, DetectionError> {
        Ok(Self {
            state: LinguaDetectionServiceState::Microsoft(DetectionService::new()?),
        })
    }

    pub fn recognize_text(
        &self,
        text_utf16: &[u16],
    ) -> Result<Vec<DetectedLanguage>, DetectionError> {
        match &self.state {
            #[cfg(feature = "lingua")]
            LinguaDetectionServiceState::Lingua(detector) => {
                let text = String::from_utf16_lossy(text_utf16);
                let result = detector.detect_multiple_languages_of(text.as_str());
                Ok(result
                    .into_iter()
                    .map(|detected| {
                        let start = text[..detected.start_index()].encode_utf16().count();
                        let len = text[detected.start_index()..detected.end_index()]
                            .encode_utf16()
                            .count();
                        let end = start + len - 1;
                        DetectedLanguage {
                            start,
                            end,
                            languages: vec![detected.language().iso_code_639_1().to_string()],
                        }
                    })
                    .collect())
            }
            LinguaDetectionServiceState::Microsoft(detection_service) => {
                detection_service.recognize_text(text_utf16)
            }
        }
    }
}
