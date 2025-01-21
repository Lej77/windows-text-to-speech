//! Defines a COM Server that offers a text-to-speech engine for Windows.

use std::time::Duration;

use windows::{
    core::{Interface, GUID, HSTRING},
    Media::{
        Playback::{MediaPlayer, MediaPlayerAudioCategory, MediaPlayerState},
        SpeechSynthesis::SpeechSynthesizer,
    },
    Storage::Streams::{DataReader, IInputStream, IRandomAccessStream},
    Win32::{
        Media::{
            Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM},
            Speech::{
                ISpObjectToken, ISpTTSEngineSite, SPVES_ABORT, SPVES_CONTINUE, SPVES_RATE,
                SPVES_SKIP, SPVES_VOLUME,
            },
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
    voices::{ParentRegKey, VoiceAttributes, VoiceKeyData},
    SafeTtsEngine, SpeechFormat, TextFrag, TextFragIter,
};

fn sapi_rate_to_modern(sapi_rate: i32) -> f64 {
    match sapi_rate.cmp(&0) {
        std::cmp::Ordering::Less => 1.0 - (sapi_rate.abs() as f64 / 20.0).clamp(0., 0.5),
        std::cmp::Ordering::Equal => 1.0,
        std::cmp::Ordering::Greater => 1.0 + (sapi_rate as f64 / 2.0).clamp(0.0, 5.0),
    }
}
fn sapi_volume_to_modern(sapi_volume: u16) -> f64 {
    (sapi_volume as f64 / 100.0).clamp(0.0, 1.0)
}

pub struct OurTtsEngine {
    /// Don't write audio to [`ISpTTSEngineSite`], instead play it directly on
    /// the audio output device. If `true` then the client application can't
    /// save the audio to a file.
    play_audio_directly: bool,
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

        let detected_language_ranges = DetectionService::new()
            .expect("Failed to find language detection service")
            .recognize_text(&text_utf16)
            .expect("Failed to recognize text language");
        log::debug!("Speak - Detected languages");

        for lang_range in detected_language_ranges {
            let text_utf16 = &text_utf16[lang_range.start..=lang_range.end];
            let synth = SpeechSynthesizer::new()?;
            let mut selected_voice = synth.Voice()?;
            let mut selected_priority = selected_voice
                .Language()
                .ok()
                .and_then(|lang| lang_range.get_priority(&lang.to_string_lossy()))
                .unwrap_or(usize::MAX);

            for voice in SpeechSynthesizer::AllVoices()? {
                let priority = voice
                    .Language()
                    .ok()
                    .and_then(|lang| lang_range.get_priority(&lang.to_string_lossy()))
                    .unwrap_or(usize::MAX);
                if priority < selected_priority {
                    selected_voice = voice;
                    selected_priority = priority;
                }
            }

            log::debug!(
                "Speak - Selected voice\n\tLanguages: {:?}\n\tVoice: {}",
                lang_range.languages,
                selected_voice
                    .DisplayName()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_else(|_| "unnamed".to_owned())
            );

            if let Err(e) = synth.SetVoice(&selected_voice) {
                log::debug!("Failed to set voice: {e}");
            }

            let synth_options = synth.Options()?;
            synth_options
                .SetSpeakingRate(sapi_rate_to_modern(unsafe { output_site.GetRate() }?))?;
            synth_options
                .SetAudioVolume(sapi_volume_to_modern(unsafe { output_site.GetVolume()? }))?;

            let stream = synth
                .SynthesizeTextToStreamAsync(&HSTRING::from_wide(text_utf16))?
                .get()?;

            enum Output<'a> {
                Player(MediaPlayer),
                Data(&'a [u16]),
            }
            let mut buffer;
            let mut output = if self.play_audio_directly {
                let rand_stream: IRandomAccessStream = stream.cast()?;

                let player = MediaPlayer::new()?;
                player.SetRealTimePlayback(true)?;
                player.SetAudioCategory(MediaPlayerAudioCategory::Speech)?;
                player.SetStreamSource(&rand_stream)?;
                player.Play()?;

                Output::Player(player)
            } else {
                let size = stream.Size()? as u32;
                let stream: IInputStream = stream.cast()?;
                let reader = DataReader::CreateDataReader(&stream)?;
                reader.LoadAsync(size)?.get()?;

                buffer = vec![0_u16; size as usize / 2];
                reader.ReadBytes(unsafe { buffer.as_mut_slice().align_to_mut::<u8>().1 })?;

                // Discard .wav header (44 bytes)
                Output::Data(&buffer[44..])
            };

            loop {
                match &mut output {
                    Output::Player(player) => {
                        let state = player.CurrentState()?;
                        if let MediaPlayerState::Stopped | MediaPlayerState::Paused = state {
                            break;
                        }

                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Output::Data(buffer) => {
                        let written_bytes = unsafe {
                            output_site
                                .Write(buffer.as_ptr().cast(), (buffer.len() * 2).min(4096) as u32)
                        }?;
                        *buffer = &buffer[written_bytes as usize / 2..];
                        if buffer.is_empty() {
                            break;
                        }
                    }
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
                // TODO: the following actions change the synthesizer settings
                // but that doesn't affect already queued sound.
                if SPVES_SKIP.0 & actions != 0 {
                    log::trace!("Skip actions are not implemented");
                }
                if SPVES_RATE.0 & actions != 0 {
                    // -10 to 10
                    let new_rate = unsafe { output_site.GetRate() }?;
                    let modern_rate = sapi_rate_to_modern(new_rate);
                    log::trace!("New SAPI rate of {new_rate} -> modern rate of {modern_rate}");
                    synth_options.SetSpeakingRate(modern_rate)?;
                }
                if SPVES_VOLUME.0 & actions != 0 {
                    // 0 to 100
                    let new_volume = unsafe { output_site.GetVolume() }?;
                    let modern_volume = sapi_volume_to_modern(new_volume);
                    log::trace!(
                        "New SAPI volume of {new_volume} -> modern volume of {modern_volume}"
                    );
                    synth_options.SetAudioVolume(modern_volume)?;
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

        // SPSF_16kHz16BitMono (16kHz 16Bit mono)
        let nSamplesPerSec = 16_000;
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
        key_name: "Lej77_TTS_Multilingual".to_owned(),
        long_name: "Lej77 - Multilingual".to_owned(),
        class_id: CLSID_OUR_TTS_ENGINE,
        attributes: VoiceAttributes {
            name: "Multilingual".to_owned(),
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
pub const CLSID_OUR_TTS_ENGINE: GUID = GUID::from_u128(0xF91EF41B_D593_442E_8730_064336410310);

struct TtsComServer;
impl SafeTtsComServer for TtsComServer {
    const CLSID_TTS_ENGINE: GUID = CLSID_OUR_TTS_ENGINE;

    type TtsEngine = OurTtsEngine;

    fn create_engine() -> Self::TtsEngine {
        OurTtsEngine {
            play_audio_directly: false,
        }
    }

    fn initialize() {
        static DLL_LOGGER: DllLogger = DllLogger::new();
        DLL_LOGGER.install()
    }

    fn register_server() {
        ComClassInfo {
            clsid: CLSID_OUR_TTS_ENGINE,
            class_name: Some("windows_tts_engine".into()),
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

        ComClassInfo::unregister_class_id(CLSID_OUR_TTS_ENGINE)
            .expect("Failed to unregister text-to-speech engine's COM Class");
    }
}

// Export the trait functions from the DLL:
dll_export_com_server_fns!(TtsComServer);
