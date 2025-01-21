//! Defines a COM Server that offers a text-to-speech engine for Windows.

use std::{
    collections::HashMap, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, sync::Mutex,
    time::Instant,
};

use piper_rs::synth::PiperSpeechSynthesizer;
use rodio::buffer::SamplesBuffer;
use windows::{
    core::GUID,
    Win32::{
        Foundation::MAX_PATH,
        Media::{
            Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM},
            Speech::{ISpObjectToken, ISpTTSEngineSite, SPVES_ABORT, SPVES_CONTINUE},
        },
        System::Registry::HKEY_LOCAL_MACHINE,
    },
};
use windows_tts_engine::{
    com_server::{
        dll_export_com_server_fns, ComClassInfo, ComServerPath, ComThreadingModel, SafeTtsComServer,
    },
    detect_languages::DetectionService,
    logging::DllLogger,
    utils::get_current_dll_path,
    voices::{ParentRegKey, VoiceAttributes, VoiceKeyData},
    SafeTtsEngine, SpeechFormat, TextFrag, TextFragIter,
};

/// Copied from [`piper_rs::Language`] since its fields aren't public.
#[derive(Clone, serde::Deserialize, Default)]
pub struct Language {
    pub code: String,
    pub family: Option<String>,
    pub region: Option<String>,
    pub name_native: Option<String>,
    pub name_english: Option<String>,
}

/// Copied from [`piper_rs::ModelConfig`] since the fields of
/// [`piper_rs::Language`] were not public.
#[derive(serde::Deserialize, Default)]
pub struct ModelConfig {
    pub key: Option<String>,
    pub language: Option<Language>,
    pub audio: piper_rs::AudioConfig,
    pub num_speakers: u32,
    pub speaker_id_map: HashMap<String, i64>,
}

pub struct PiperModelInfo {
    /// Path to JSON config.
    pub path: PathBuf,
    pub language: Option<Language>,
}

pub struct OurTtsEngine {
    /// Don't write audio to [`ISpTTSEngineSite`], instead play it directly on
    /// the audio output device. If `true` then the client application can't
    /// save the audio to a file.
    play_audio_directly: bool,
    cache: Mutex<HashMap<PathBuf, PiperSpeechSynthesizer>>,
}
impl OurTtsEngine {
    pub fn list_models(&self) -> Option<Vec<PiperModelInfo>> {
        let start_finding = Instant::now();

        let mut model_folder = {
            let mut buf = [0; MAX_PATH as _];
            PathBuf::from(<OsString as OsStringExt>::from_wide(
                get_current_dll_path(&mut buf)
                    .map_err(|e| log::error!("Failed to get dll path: {e}"))
                    .ok()?
                    .strip_suffix(&[0])
                    .expect("nul terminator"),
            ))
        };
        model_folder.pop();
        model_folder.push("piper_models");
        if !model_folder.is_dir() {
            log::warn!("No folder for piper models at: {}", model_folder.display());
            return None;
        }

        let mut models = Vec::new();
        for entry in std::fs::read_dir(&model_folder)
            .map_err(|e| log::error!("Failed to list entries in model folder: {e}"))
            .ok()?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    log::warn!("Failed to get model folder entry: {e}");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext != "json") || !path.is_file() {
                log::debug!(
                    "Skipped file inside piper_models folder: {}",
                    path.display()
                );
                continue;
            }
            let data = match std::fs::read(&path) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("Failed to read model config at \"{}\": {e}", path.display());
                    continue;
                }
            };
            let config = match serde_json::from_slice::<ModelConfig>(&data) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize model config at \"{}\": {e}",
                        path.display()
                    );
                    continue;
                }
            };
            models.push(PiperModelInfo {
                path,
                language: config.language,
            })
        }
        if models.is_empty() {
            log::warn!(
                "No piper models inside folder at: {}",
                model_folder.display()
            );
            return None;
        }
        log::debug!(
            "Finding all model files took: {:?}",
            start_finding.elapsed()
        );

        Some(models)
    }
    pub fn voice_to_select(&self, mut config_path: PathBuf) -> Option<i64> {
        config_path.set_extension("");
        config_path.set_extension("voice.txt");
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| {
                log::warn!(
                    "Failed to read voice.txt info at \"{}\": {e}",
                    config_path.display()
                )
            })
            .ok()?;
        content
            .trim()
            .parse::<i64>()
            .map_err(|e| log::error!("Speaker ID should be number: {e}"))
            .ok()
    }
}
impl SafeTtsEngine for OurTtsEngine {
    fn set_object_token(&self, _token: &ISpObjectToken) -> windows::core::Result<()> {
        log::debug!("set_object_token");
        Ok(())
    }

    fn speak(
        &self,
        _token: &ISpObjectToken,
        _speak_punctuation: bool,
        _wave_format: SpeechFormat,
        text_fragments: Option<TextFrag<'_>>,
        output_site: &ISpTTSEngineSite,
    ) -> windows::core::Result<()> {
        let text_utf16 = TextFragIter::new(text_fragments)
            .flat_map(|frag| frag.utf16_text().iter().copied().chain([' ' as u16]))
            .collect::<Vec<u16>>();
        let all_text = String::from_utf16_lossy(&text_utf16);
        log::debug!("Speak: {all_text}");

        let Some(models) = self.list_models() else {
            return Ok(());
        };

        let detected_language_ranges = DetectionService::new()
            .expect("Failed to find language detection service")
            .recognize_text(&text_utf16)
            .expect("Failed to recognize text language");
        log::debug!("Speak - Detected languages");

        for lang_range in detected_language_ranges {
            let text_utf16 = &text_utf16[lang_range.start..=lang_range.end];

            let preferred_model = models
                .iter()
                .min_by_key(|model| {
                    model
                        .language
                        .as_ref()
                        .and_then(|lang| lang_range.get_priority(&lang.code))
                        .unwrap_or(usize::MAX)
                })
                .expect("There are at least one model");

            let model = {
                let mut guard = self.cache.lock().unwrap();
                if let Some(synth) = guard.get(&preferred_model.path) {
                    synth.clone_model()
                } else {
                    let start_read = Instant::now();
                    let model = piper_rs::from_config_path(&preferred_model.path)
                        .expect("Failed to load piper config");
                    log::debug!("Reading the model took: {:?}", start_read.elapsed());

                    guard.insert(
                        preferred_model.path.clone(),
                        PiperSpeechSynthesizer::new(model.clone())
                            .expect("Failed to create piper synthesizer"),
                    );
                    model
                }
            };

            let _start_audio = Instant::now();

            let audio_info = model
                .audio_output_info()
                .expect("failed to get audio format info");

            // Set speaker ID
            if let Some(sid) = self.voice_to_select(preferred_model.path.clone()) {
                if let Some(e) = model.set_speaker(sid) {
                    log::error!("Failed to set speaker: {e}");
                }
            }
            let synth =
                PiperSpeechSynthesizer::new(model).expect("Failed to create piper synthesizer");
            let audio = synth
                .synthesize_parallel(String::from_utf16_lossy(text_utf16), None)
                .expect("Failed to synthesize audio using piper");

            log::debug!("Piper generating audio with: {audio_info:?}");

            if self.play_audio_directly
                || audio_info.sample_rate != 22050
                || audio_info.num_channels != 1
                || audio_info.sample_width != 2
            {
                if !self.play_audio_directly {
                    log::warn!("Fallback to direct audio output since this model uses an uncommon audio format");
                }
                #[cfg(feature = "direct_output")]
                {
                    let mut samples: Vec<f32> = Vec::new();
                    for result in audio {
                        samples.append(&mut result.expect("Failed to generate samples").into_vec());
                    }
                    log::debug!(
                        "Generating the audio data took: {:?}",
                        _start_audio.elapsed()
                    );

                    let (_stream, handle) = rodio::OutputStream::try_default()
                        .expect("Failed to create audio output stream");
                    let sink = rodio::Sink::try_new(&handle).unwrap();

                    let buf = SamplesBuffer::new(1, 22050, samples);
                    sink.append(buf);

                    sink.sleep_until_end();
                }
            } else {
                let mut samples = Vec::new();
                for result in audio {
                    samples
                        .append(&mut result.expect("Failed to generate samples").as_wave_bytes());
                }
                let mut buffer = samples.as_slice();
                loop {
                    let written_bytes = unsafe {
                        output_site.Write(buffer.as_ptr().cast(), buffer.len().min(4096) as u32)
                    }?;
                    buffer = &buffer[written_bytes as usize..];
                    if buffer.is_empty() {
                        break;
                    }

                    // Call GetActions as often as possible (returns bitflags):
                    // https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee431802(v=vs.85)
                    let actions = unsafe { output_site.GetActions() } as i32;
                    if actions == SPVES_CONTINUE.0 {
                        continue;
                    }
                    if SPVES_ABORT.0 & actions != 0 {
                        return Ok(());
                    }
                    // TODO: handle other actions
                }
            }
        }

        Ok(())
    }

    #[expect(non_snake_case)]
    fn get_output_format(
        &self,
        _token: &ISpObjectToken,
        target_format: Option<SpeechFormat>,
    ) -> windows::core::Result<SpeechFormat> {
        log::debug!("get_output_format: {target_format:?}");
        if let Some(SpeechFormat::DebugText) = target_format {
            return Ok(SpeechFormat::DebugText);
        }

        // SPSF_16kHz16BitMono (22kHz 16Bit mono)
        // TODO: some models have other output formats
        let nSamplesPerSec = 22050;
        let nBlockAlign = 2;
        Ok(SpeechFormat::Wave(WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as _,
            nChannels: 1,
            nBlockAlign,
            wBitsPerSample: 16,
            nSamplesPerSec,
            nAvgBytesPerSec: nSamplesPerSec * (nBlockAlign as u32),
            cbSize: 0,
        }))
    }
}

fn multilingual_voice_data() -> VoiceKeyData {
    VoiceKeyData {
        key_name: "Lej77_TTS_PIPER_MULTILINGUAL".to_owned(),
        long_name: "Lej77 - Piper - Multilingual".to_owned(),
        class_id: CLSID_PIPER_TTS_ENGINE,
        attributes: VoiceAttributes {
            name: "Piper Multilingual".to_owned(),
            gender: "Male".to_owned(),
            age: "Adult".to_owned(),
            language: "409".to_owned(), // en-US
            vendor: "Lej77 at GitHub".to_owned(),
        },
    }
}

/// The "class ID" this text-to-speech engine is identified by. This value needs
/// to match the value used when registering the engine to the Windows registry.
///
/// This unique id was generated using `uuidgen.exe`.
pub const CLSID_PIPER_TTS_ENGINE: GUID = GUID::from_u128(0x9876903A_2109_4BCC_A64B_242880E12AD2);

struct TtsComServer;
impl SafeTtsComServer for TtsComServer {
    const CLSID_TTS_ENGINE: GUID = CLSID_PIPER_TTS_ENGINE;

    type TtsEngine = OurTtsEngine;

    fn create_engine() -> Self::TtsEngine {
        OurTtsEngine {
            play_audio_directly: false,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn initialize() {
        static DLL_LOGGER: DllLogger = DllLogger::new();
        DLL_LOGGER.install()
    }

    fn register_server() {
        ComClassInfo {
            clsid: CLSID_PIPER_TTS_ENGINE,
            class_name: Some("windows_tts_engine_piper".into()),
            threading_model: ComThreadingModel::Apartment,
            server_path: ComServerPath::CurrentModule,
        }
        .register()
        .expect("Failed to register COM Class");

        let voice = multilingual_voice_data();
        voice
            .write_to_registry(ParentRegKey::Path(
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Speech\\Voices\\Tokens\\",
            ))
            .expect("Failed to register multilingual voice");
        voice
            .write_to_registry(ParentRegKey::Path(
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Speech_OneCore\\Voices\\Tokens\\",
            ))
            .expect("Failed to register multilingual data to modern voice path");
    }

    fn unregister_server() {
        let voice = multilingual_voice_data();
        voice
            .remove_from_registry(ParentRegKey::Path(
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Speech_OneCore\\Voices\\Tokens\\",
            ))
            .expect("Failed to unregister multilingual data from modern voice path");
        voice
            .remove_from_registry(ParentRegKey::Path(
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Speech\\Voices\\Tokens\\",
            ))
            .expect("Failed to unregister multilingual voice");

        ComClassInfo::unregister_class_id(CLSID_PIPER_TTS_ENGINE)
            .expect("Failed to unregister text-to-speech engine's COM Class");
    }
}

// Export the trait functions from the DLL:
dll_export_com_server_fns!(TtsComServer);
