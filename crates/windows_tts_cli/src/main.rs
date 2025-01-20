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

use std::{marker::PhantomData, path::PathBuf, ptr::null_mut, time::Duration};

use anyhow::{bail, Context};
use clap::Parser;
use windows::{
    core::{Interface, GUID, HSTRING, PCWSTR},
    Media::{
        Playback::{MediaPlayer, MediaPlayerAudioCategory, MediaPlayerState},
        SpeechSynthesis::{SpeechSynthesizer, VoiceInformation},
    },
    Storage::Streams::{DataReader, IInputStream, IRandomAccessStream},
    Win32::{
        Globalization::{
            MappingFreePropertyBag, MappingFreeServices, MappingGetServices, MappingRecognizeText,
            ELS_GUID_LANGUAGE_DETECTION, MAPPING_ENUM_OPTIONS, MAPPING_PROPERTY_BAG,
            MAPPING_SERVICE_INFO,
        },
        Media::Speech::{
            ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SpObjectTokenCategory, SpVoice,
            SPCAT_VOICES,
        },
        System::Com::{CoCreateInstance, CoInitialize, CoTaskMemFree, CoUninitialize, CLSCTX_ALL},
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

/// If an instance of this type exists then it is a promise that the COM library
/// is initialized on the current thread.
pub struct HasCoInitialized {
    /// Marks this type as **not** thread-safe since we need to uninitialize the COM
    /// library on the same thread we created initialized it from.
    marker: PhantomData<*mut ()>,
}
impl HasCoInitialized {
    pub fn new() -> windows::core::Result<Self> {
        unsafe { CoInitialize(None) }.ok()?;
        Ok(Self {
            marker: PhantomData,
        })
    }
    /// Promise that the COM library is initialized for the current thread.
    pub fn new_unchecked() -> &'static Self {
        // Note: leaking zero sized type won't actually leak anything and since
        // the object won't be dropped we won't call `CoUninitialize`.
        Box::leak(Box::new(Self {
            marker: PhantomData,
        }))
    }
}
impl Drop for HasCoInitialized {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCategoryId {
    Default,
    Modern,
}
impl VoiceCategoryId {
    fn create_category_token_with_id(self) -> anyhow::Result<ISpObjectTokenCategory> {
        let otc: ISpObjectTokenCategory =
            unsafe { CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL) }
                .context("Failed to CoCreateInstance of ISpObjectTokenCategory")?;

        let category_id = match self {
            VoiceCategoryId::Default => SPCAT_VOICES,
            VoiceCategoryId::Modern => {
                windows::core::w!("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Speech_OneCore\\Voices")
            }
        };
        unsafe { otc.SetId(PCWSTR::from_raw(category_id.as_ptr()), false) }
            .context("Failed to SetId for ISpObjectTokenCategory")?;

        Ok(otc)
    }

    /// Enumerates all voices
    ///
    /// # References
    ///
    /// Code was inspired by answer to this question:
    /// <https://learn.microsoft.com/en-sg/answers/questions/2006006/would-copying-registry-entries-to-get-access-to-al>
    ///
    /// More info about enumerating voices at:
    /// [Object Tokens and Registry Settings (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms717036(v=vs.85))
    pub fn enum_voices(self) -> anyhow::Result<Vec<ISpObjectToken>> {
        let otc: ISpObjectTokenCategory = self.create_category_token_with_id()?;

        let voices = unsafe { otc.EnumTokens(PCWSTR::null(), PCWSTR::null()) }
            .context("Failed to EnumTokens for ISpObjectTokenCategory")?;

        let mut count = 0;
        unsafe { voices.GetCount(&mut count) }
            .context("Failed to GetCount for ISpObjectTokenCategory")?;

        Ok((0..count)
            .map(|index| unsafe { voices.Item(index) })
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// This doesn't work correctly for [`VoiceCategoryId::Modern`].
    pub fn default_voice_id(self) -> anyhow::Result<String> {
        let otc: ISpObjectTokenCategory = self.create_category_token_with_id()?;

        let token_id = unsafe { otc.GetDefaultTokenId() }
            .context("Failed to call GetDefaultTokenId for ISpObjectTokenCategory")?;

        if token_id.is_null() {
            bail!("No default voice token");
        }

        let token_id_str = unsafe { token_id.to_string() };

        unsafe { CoTaskMemFree(Some(token_id.as_ptr().cast())) };

        Ok(token_id_str?)
    }
}

/// This speaks some text aloud.
///
/// Note that this will use the legacy voices at [`SPCAT_VOICES`] (from
/// [`VoiceCategoryId::Default`]) if no `voice_token` is specified. This default
/// voice can be changed from Windows' Control Panel, not from the modern
/// Settings app.
pub fn speak(text_utf16: &[u16], voice_token: Option<&ISpObjectToken>) -> anyhow::Result<()> {
    let voice: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }
        .context("Failed to CoCreateInstance of ISpVoice")?;

    if let Some(voice_token) = voice_token {
        unsafe { voice.SetVoice(voice_token) }.context("Failed to set voice")?;
    }

    unsafe { voice.Speak(PCWSTR::from_raw(text_utf16.as_ptr()), 0, None) }
        .context("Failed to call ISpVoice::Speak")?;

    Ok(())
}

fn print_legacy_voices() -> anyhow::Result<()> {
    for category_id in [VoiceCategoryId::Default, VoiceCategoryId::Modern] {
        println!(
            "\nAll voices found using legacy API ({category_id:?} voice category registry key):"
        );

        let voices = category_id
            .enum_voices()
            .context("Failed to enumerate voices")?;

        println!(
            "Default voice{}: {}",
            if category_id == VoiceCategoryId::Modern {
                " (incorrect)"
            } else {
                ""
            },
            category_id.default_voice_id()?
        );

        for voice in &voices {
            println!("Voice Id: {}", unsafe { voice.GetId()?.to_string()? });
        }
        println!("\n");
    }
    Ok(())
}

/// Uses Windows APIs for text-to-speech.
#[derive(Parser)]
struct Args {
    /// Skip the legacy text-to-speech output.
    #[clap(long)]
    no_legacy: bool,

    /// Skip the modern text-to-speech output.
    #[clap(long)]
    no_modern: bool,

    /// Write modern text-to-speech output to a file.
    #[clap(long)]
    write_modern_to_file: Option<PathBuf>,

    /// Print info about all installed voices.
    #[clap(long)]
    print_all_voices: bool,

    /// Path to piper model config.
    ///
    /// If you download a model using:
    ///
    /// wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx
    ///
    /// wget https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/en_US-libritts_r-medium.onnx.json
    ///
    /// Then you can provide the following config path:
    /// `en_US-libritts_r-medium.onnx.json`
    #[cfg(feature = "piper-rs")]
    #[clap(long)]
    piper_config_path: Option<std::path::PathBuf>,

    /// Text that should be converted to speech.
    text: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let text = args.text.join(" ");
    if text.is_empty() {
        bail!("Should specify text to read as command line arguments");
    }
    println!("Text-to-speech for:\n{text}\n");

    let text_utf16 = to_utf16(&text);

    let _com_init =
        HasCoInitialized::new().context("Failed to initialize COM library for current thread")?;

    // Legacy SAPI:
    if !args.no_legacy {
        if args.print_all_voices {
            print_legacy_voices()?;
        }

        speak(&text_utf16, None)?;

        println!("Finished with legacy voice output\n");
    }

    if !args.no_modern {
        if !is_windows_10()? {
            eprintln!("Modern text-to-speech API is only available in Windows 10 or newer");
            std::process::exit(2);
        }

        let detected_language_ranges = DetectionService::new()
            .context("Failed to find language detection service")?
            .recognize_text(&text_utf16)
            .context("Failed to recognize text language")?;

        println!(
            "Count of detected Language ranges: {}",
            detected_language_ranges.len()
        );
        for lang_detection in detected_language_ranges {
            let text_utf16 = &text_utf16[lang_detection.start..=lang_detection.end];
            println!(
                "First range of text ({}-{}): {}",
                lang_detection.start,
                lang_detection.end,
                String::from_utf16_lossy(text_utf16)
            );
            println!(
                "\tDetected possible languages (prefer earlier ones): {:?}",
                lang_detection.languages
            );

            let synth = SpeechSynthesizer::new()?;
            let default_voice = synth.Voice()?;
            let all_voices = SpeechSynthesizer::AllVoices()?;

            if args.print_all_voices {
                println!("\nAll voices:");
                for voice in &all_voices {
                    println!("Voice: {}", voice.DisplayName()?.to_string_lossy());
                    println!("\tid: {}", voice.Id()?.to_string_lossy());
                    println!("\tLang: {}", voice.Language()?.to_string_lossy());
                    println!();
                }
            }

            'find_lang: for wanted_lang in &lang_detection.languages {
                let right_lang = |voice: &VoiceInformation| -> anyhow::Result<bool> {
                    Ok(voice
                        .Language()?
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&wanted_lang.to_lowercase()))
                };

                if right_lang(&default_voice)? {
                    println!(
                        "Default voice \"{}\" matches the wanted language",
                        default_voice.DisplayName()?.to_string_lossy()
                    );
                    break;
                } else {
                    println!(
                        "Default voice doesn't match language {wanted_lang}, find one that does"
                    );

                    for voice in &all_voices {
                        if right_lang(&voice)? {
                            println!("Selected voice: {}", voice.DisplayName()?.to_string_lossy());
                            synth.SetVoice(&voice)?;
                            break 'find_lang; // Break out of two loops
                        }
                    }
                }

                println!(
                    "No voice for the detected language \"{wanted_lang}\", \
                    checking for less likely languages"
                );
            }
            println!();

            let stream = synth
                .SynthesizeTextToStreamAsync(&HSTRING::from_wide(text_utf16))?
                .get()?;
            println!("Stream context type: {}", stream.ContentType()?);
            if let Some(file_path) = &args.write_modern_to_file {
                // https://stackoverflow.com/questions/59061345/how-to-save-speechsynthesis-audio-to-a-mp3-file-in-a-uwp-application
                // https://stackoverflow.com/questions/65737953/how-to-save-audio-from-using-windows-media-speechsynthesis
                // https://www.codeproject.com/Articles/1067252/Tackling-text-to-speech-and-generating-audio-file

                let size = stream.Size()? as u32;
                let stream: IInputStream = stream.cast()?;
                let reader = DataReader::CreateDataReader(&stream)?;
                reader.LoadAsync(size)?.get()?;

                let mut buffer = vec![0; size as usize];
                reader.ReadBytes(buffer.as_mut_slice())?;

                std::fs::write(file_path.with_extension(".wav"), buffer)?;
            } else {
                let stream: IRandomAccessStream = stream.cast()?;

                let player = MediaPlayer::new()?;
                player.SetRealTimePlayback(true)?;
                player.SetAudioCategory(MediaPlayerAudioCategory::Speech)?;
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
        }

        println!("Finished with modern voice output\n");
    }

    #[cfg(feature = "natural-tts")]
    {
        use natural_tts::{models::msedge::MSEdgeModel, *};

        // Create the NaturalTts struct using the builder pattern.
        let mut natural = NaturalTtsBuilder::default()
            .msedge_model(MSEdgeModel::default())
            .default_model(Model::MSEdge)
            .build()?;

        // Use the pre-included function to say a message using the default_model.
        let _: () = natural
            .say_auto(text.clone())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        std::thread::sleep(Duration::from_millis(1000));

        println!("Finished with MSEdge voice output\n");
    }

    #[cfg(feature = "tts_rust")]
    {
        use tts_rust::tts::GTTSClient;

        let narrator: GTTSClient = GTTSClient::default();
        narrator
            .speak(&text[..text.len().min(50)])
            .map_err(|e| anyhow::anyhow!(e))?;
        println!("Finished with Google text-to-speech output (from cloud service)\n");
    }

    #[cfg(feature = "piper-rs")]
    {
        use piper_rs::synth::PiperSpeechSynthesizer;
        use rodio::buffer::SamplesBuffer;

        let model =
            piper_rs::from_config_path(args.piper_config_path.as_ref().context(
                "Piper TTS doesn't work unless --piper-config-path argument is specified",
            )?)
            .context("Failed to load piper config")?;
        // Set speaker ID
        // if let Some(sid) = sid {
        //     let sid = sid.parse::<i64>().expect("Speaker ID should be number!");
        //     model.set_speaker(sid);
        // }
        let synth =
            PiperSpeechSynthesizer::new(model).context("Failed to create piper synthesizer")?;
        let mut samples: Vec<f32> = Vec::new();
        let audio = synth
            .synthesize_parallel(text, None)
            .context("Failed to synthesize audio using piper")?;
        for result in audio {
            samples.append(&mut result.unwrap().into_vec());
        }

        let (_stream, handle) =
            rodio::OutputStream::try_default().context("Failed to create audio output stream")?;
        let sink = rodio::Sink::try_new(&handle).unwrap();

        let buf = SamplesBuffer::new(1, 22050, samples);
        sink.append(buf);

        sink.sleep_until_end();
        println!("Finished with Piper neural network text-to-speech model\n");
    }

    Ok(())
}
