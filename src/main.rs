//! # References
//!
//! - There are two APIs: [text to speech - Windows 10 TTS voices not showing up? - Stack
//!   Overflow](https://stackoverflow.com/questions/40406719/windows-10-tts-voices-not-showing-up/40427509#40427509)
//! - Legacy API: [Text-to-Speech Tutorial (SAPI 5.3) | Microsoft
//!   Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720163(v=vs.85))
//! - Modern API: [Windows.Media.SpeechSynthesis Namespace - Windows apps | Microsoft
//!   Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis?view=winrt-26100&redirectedfrom=MSDN)
//! - Detect language: [Microsoft Language Detection - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/intl/microsoft-language-detection)
//!   - [About Extended Linguistic Services - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/about-extended-linguistic-services)
//!   - [Requesting Text Recognition - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/requesting-text-recognition)

use std::{ptr::null_mut, time::Duration};

use anyhow::{bail, Context};
use windows::{
    core::{Interface, GUID, HSTRING, PCWSTR},
    Media::{
        Playback::{MediaPlayer, MediaPlayerState},
        SpeechSynthesis::{SpeechSynthesizer, VoiceInformation},
    },
    Storage::Streams::IRandomAccessStream,
    Win32::{
        Globalization::{
            MappingFreePropertyBag, MappingFreeServices, MappingGetServices, MappingRecognizeText,
            ELS_GUID_LANGUAGE_DETECTION, MAPPING_ENUM_OPTIONS, MAPPING_PROPERTY_BAG,
            MAPPING_SERVICE_INFO,
        },
        Media::Speech::{ISpVoice, SpVoice},
        System::Com::{CoCreateInstance, CoInitialize, CoUninitialize, CLSCTX_ALL},
    },
};

pub fn to_utf16(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    OsStr::new(s)
        .encode_wide()
        .chain(core::iter::once(0u16))
        .collect()
}

pub fn is_windows_10() -> anyhow::Result<bool> {
    let mut version: windows::Win32::System::SystemInformation::OSVERSIONINFOW = Default::default();
    version.dwOSVersionInfoSize = core::mem::size_of_val(&version) as u32;
    unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut version) }
        .ok()
        .context("Failed to determine Windows version")?;

    // SpeechSynthesizer class was introduced in build 10240 (the original Windows 10 version), see:
    // https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizer?view=winrt-26100
    // https://en.wikipedia.org/wiki/Windows_10_(original_release)

    Ok(version.dwBuildNumber >= 10240)
}

pub struct DetectedLanguage {
    pub start: usize,
    pub end: usize,
    pub languages: Vec<String>,
}

/// Language detection service info.
pub struct DetectionService {
    service: *mut MAPPING_SERVICE_INFO,
}
impl DetectionService {
    pub fn new() -> anyhow::Result<Self> {
        // Can use utf16 category but we use GUID directly
        // let mut _category = to_utf16("Language Detection");

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
            .context("MappingGetServices failed")?;

        // This object will call `MappingFreeServices` later:
        let service = DetectionService {
            service: services_ptr,
        };
        let services = unsafe { std::slice::from_raw_parts(services_ptr, len as usize) };
        let first = services[0];
        if first.guid != ELS_GUID_LANGUAGE_DETECTION {
            bail!("Incorrect GUID for language detection service");
        }
        // for service in services {
        //     eprintln!("Service info: {}", unsafe {
        //         service.pszDescription.to_string()?
        //     });
        // }
        if len != 1 {
            bail!("More than one Language Detection service found");
        }
        Ok(service)
    }

    pub fn recognize_text(&self, text_utf16: &[u16]) -> anyhow::Result<Vec<DetectedLanguage>> {
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
        .context("MappingRecognizeText")?;

        let _service_data = unsafe {
            std::slice::from_raw_parts(
                prop_bag.pServiceData as *const u16,
                prop_bag.dwServiceDataSize as usize / 2,
            )
        };
        // eprintln!("Recognize language service data: {:?}", _service_data);

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
                .expect("no trailing nul") // two trailing nul characters
                .split(|&v| v == 0) // then one nul between every two detected langs
                .map(String::from_utf16) // text is utf16 encoded
                .collect::<Result<Vec<String>, _>>()?;
            // for lang in &languages {
            //     eprintln!(
            //         "Detected language ({}-{}): {}",
            //         range.dwStartIndex, range.dwEndIndex, lang
            //     );
            // }
            // eprintln!("\tContentType: {}", unsafe {
            //     range.pszContentType.to_string()
            // }?);
            detected.push(DetectedLanguage {
                start: range.dwStartIndex as usize,
                end: range.dwEndIndex as usize,
                languages,
            })
        }

        unsafe { MappingFreePropertyBag(&prop_bag) }.context("MappingFreePropertyBag")?;
        Ok(detected)
    }
}
impl Drop for DetectionService {
    fn drop(&mut self) {
        unsafe { MappingFreeServices(self.service) }.expect("MappingFreeServices failed");
    }
}

#[pollster::main]
async fn main() -> anyhow::Result<()> {
    let text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        bail!("Should specify text to read as command line arguments");
    }
    println!("Text-to-speech for:\n{text}\n");
    let text_utf16 = to_utf16(&text);

    // Legacy SAPI:
    unsafe { CoInitialize(None).ok()? };
    let voice: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }?;
    unsafe { voice.Speak(PCWSTR::from_raw(text_utf16.as_ptr()), 0, None) }?;
    unsafe { CoUninitialize() };
    println!("Finished with legacy voice output\n");

    if !is_windows_10()? {
        eprintln!("Modern text-to-speech API is only available in Windows 10 or newer");
        std::process::exit(2);
    }

    let detected_language_ranges = {
        let detection_service =
            DetectionService::new().context("Failed to find language detection service")?;
        detection_service
            .recognize_text(&text_utf16)
            .context("Failed to recognize text language")?
    };
    println!(
        "Count of detected Language ranges: {}",
        detected_language_ranges.len()
    );
    for mut detected_language_info in detected_language_ranges {
        let text_utf16 = &text_utf16[detected_language_info.start..=detected_language_info.end];
        let detected_language = detected_language_info.languages.remove(0);
        println!(
            "First range of text ({}-{}): {}",
            detected_language_info.start,
            detected_language_info.end,
            String::from_utf16_lossy(text_utf16)
        );
        println!(
            "\tDetected language as \"{detected_language}\", secondly as: {:?}",
            detected_language_info.languages
        );

        let synth = SpeechSynthesizer::new()?;
        let default_voice = synth.Voice()?;
        let right_lang = |voice: &VoiceInformation| -> anyhow::Result<bool> {
            Ok(voice
                .Language()?
                .to_string_lossy()
                .to_lowercase()
                .contains(&detected_language.to_lowercase()))
        };

        if !right_lang(&default_voice)? {
            println!("Default voice doesn't match, find one that does");
            let all_voices = SpeechSynthesizer::AllVoices()?;

            println!("All voices:");
            for voice in &all_voices {
                println!("Voice: {}", voice.DisplayName()?.to_string_lossy());
                println!("\tid: {}", voice.Id()?.to_string_lossy());
                println!("\tLang: {}", voice.Language()?.to_string_lossy());
                println!();
            }

            for voice in &all_voices {
                if right_lang(&voice)? {
                    println!("Selected voice: {}", voice.DisplayName()?.to_string_lossy());
                    synth.SetVoice(&voice)?;
                    break;
                }
            }
        }
        let stream = synth
            .SynthesizeTextToStreamAsync(&HSTRING::from_wide(text_utf16))?
            .await?;
        let stream: IRandomAccessStream = stream.cast()?;

        let player = MediaPlayer::new()?;
        player.SetStreamSource(&stream)?;
        player.Play()?;
        loop {
            let state = player.CurrentState()?;
            if let MediaPlayerState::Stopped | MediaPlayerState::Paused = state {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    println!("Finished with modern voice output");

    Ok(())
}
