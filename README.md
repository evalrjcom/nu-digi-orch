# Nu Digi Orch

Nu Digi Orch is a lightweight desktop MIDI composer in Rust and FLTK. Arrange and edit MIDI, audition instruments on a virtual piano, and mix SoundFont tracks for stereo WAV export. You can also route the arrangement to an external MIDI device.

[Download a build from GitHub Actions](https://github.com/evalrjcom/nu-digi-orch/actions/workflows/build.yml).

## Track view and playhead

The composer opens in split view. The upper table combines track names, channel, program, bank, volume, pan, mute, and solo with the arrangement lanes. Click a row to edit that track in the piano roll below. Double-click a name or numeric cell to edit it, or click M or S to toggle mute or solo. Scroll over the table to reach additional tracks.

Choose **Full track view** for the arrangement alone. Click a beat on either ruler to cue playback or jump while playing. Play and Loop start at the cue. Use the BPM up/down arrows or click the up/down controls beside channel, program, bank, volume, or pan. Select a numeric cell to adjust it with the keyboard arrow keys.

Choose **Full piano roll** or double-click an arrangement lane to expand the piano roll across the editing area. Use the track selector beside **View** to switch tracks in Full Piano Roll. **Split view** restores the table. Both panes share the same beat grid and playback position.

During playback and recording, the amber vertical line marks the current musical position in both views. It follows tempo changes, advances to the next 16-beat page, and returns to the cue during loop playback.

| Control | Action |
| --- | --- |
| Space | Play or pause. Space remains available for typing in text fields. |
| Ctrl+Left / Ctrl+Right | Jump backward or forward five seconds per press, including while paused or stopped. |
| Stop | Stop at the current position. Click twice within 400 ms to return to the beginning. |
| Ctrl+K | Open the virtual piano. |
| Ctrl+Z / Ctrl+Y | Undo or redo. |

## Track mixer

Choose **Track mixer** in the view selector. Each strip shows the track name, MIDI channel and program. Drag the vertical volume fader or horizontal pan slider, and click Mute or Solo while playing. Scroll over the mixer to reach additional tracks. Volume and pan use standard MIDI values from 0 to 127, with pan centered at 64. SoundFont instrument tracks have independent volume, pan, bank, and program even when their MIDI channels match. External MIDI devices share these settings per channel.

## Start composing

1. Open a MIDI file with **File → Open MIDI**, or open the built-in two-track song with **File → Open Demo**.
2. Choose **Load SF2** and select a SoundFont 2 file. The software synth plays through the system default or a device selected in **Audio device**. On Linux the app also checks common system SoundFont locations. Set `NU_DIGI_ORCH_SF2` to load a specific file at startup.
3. Press **Play** or activate **Loop** before playback. To send MIDI to hardware or another application, select its port in **Output**.
4. Double-click a track lane to open its piano roll. Click the piano roll to add or select a note. Drag a note to move it in pitch and time. Right-click or press Delete with the roll focused to remove a note. The note fields below the roll set exact tick position, pitch, duration, and velocity. **Snap** controls the edit grid, and **New length** controls the duration of newly drawn notes.
5. Add tracks with **+**. Select a MIDI channel and program and set bank, volume, and pan in the track table. Channel 10 is percussion in General MIDI. Click a **Prog** value or **SoundFont presets…** to browse banks, patches, and names in the loaded SoundFont. Select a preset, then click **Apply to track**. External devices share patch and controller settings per MIDI channel. Mute and Solo control playback and WAV export for the current session.
6. Pick a port in **MIDI input…** in the upper-right corner, beside the audio device selector, then press **Record MIDI**. The app captures a take from the start of the timeline. Play or monitor the instrument on your MIDI device during capture. **Stop** adds the take as a new editable track.
7. Save a standard MIDI file, or use **File → Export WAV** to mix the audible tracks to a stereo 44.1 kHz, 16-bit WAV with the loaded SoundFont.

For longer songs, set **First beat** to move horizontally and **Top key** to change the pitch range.

### Digital Orchestrator import

**File → Open MIDI** accepts standard MIDI (`.mid`, `.midi`), RIFF MIDI (`.rmi`), and Voyetra Digital Orchestrator (`.orc`) files with an embedded RIFF MIDI sequence. ORC import brings in the MIDI arrangement, including note and patch events. Embedded audio and Voyetra-specific project settings are excluded. Save the imported arrangement as a separate `.mid` file. Voyetra track patch metadata is restored as standard GM program changes when the track has no explicit startup program.

## Sound Blaster and MIDI routing

The **Output** list uses the operating system's MIDI API (WinMM, ALSA sequencer, or CoreMIDI). Ports whose names contain common Creative/Sound Blaster identifiers are labeled **Sound Blaster**. **Playback → Refresh Devices** rescans after connecting hardware. A card or driver must expose a MIDI port to appear in the list. If a card exposes audio output only, choose it under **Audio device** and select **Software SoundFont**. Devices whose audio driver name matches a Sound Blaster identifier are labeled there too.

SF2 banks are loaded into the internal software synthesizer. To use an onboard hardware synth, configure its bank with its driver or vendor utility, then choose the MIDI port in Nu Digi Orch. A virtual MIDI port can route events to another application. The app's internal software synth works without a virtual MIDI port.

## Builds

GitHub Actions builds Windows x64, Linux x64, and macOS arm64 binaries. Download the artifact for your platform from a passing **Build composer** run and extract the archive inside it. Linux and macOS archives preserve the executable permission. The macOS binary is unsigned and runs from a terminal.

To build locally, install current stable Rust, CMake 3.28 or later, and a C++ compiler. Use Visual Studio Build Tools with Desktop development with C++ on Windows or Xcode Command Line Tools on macOS. On Debian or Ubuntu, install:

```sh
sudo apt-get install build-essential cmake pkg-config libasound2-dev libx11-dev libxext-dev \
  libxft-dev libxinerama-dev libxcursor-dev libxrender-dev libxfixes-dev libxrandr-dev libxss-dev
```

```sh
cargo test --locked
cargo run --release --locked
```

The CLI can generate the demo MIDI and render a file without opening a window:

```sh
nu-digi-orch --demo-midi demo.mid
nu-digi-orch --render demo.mid GeneralMIDI.sf2 demo.wav
```

The project format is Standard MIDI File 0 or 1 with ticks-per-quarter timing. Tempo changes, MIDI channel events, and MIDI metadata are retained when saving. SysEx and escape events are preserved in MIDI files. Playback and WAV rendering process channel messages and the tempo map. SoundFonts are supplied separately.

## SoundFont effects

Open **Playback → SoundFont effects** to adjust the master reverb, chorus and stereo feedback delay during playback. Each slider runs from 0 to 100%. The settings apply to the software synth and GUI WAV exports for this session. Playback and WAV export allow four seconds of decay after the final MIDI event when effects are active, or one second otherwise. WAV export captures the current settings. External MIDI devices use their own audio processing.

### Mix and master controls

In **Track mixer**, drag the Bass or Treble line left or right to cut or boost up to 12 dB around 200 Hz or 4 kHz. The fixed **Master** strip controls the combined SoundFont output after effects, with unity at 100% and silence at 0%. Each track and the master have a segmented stereo peak meter on a −60 to 0 dBFS scale. Green shows normal level, yellow starts above −12 dBFS, and the red **CLIP** light holds for one second after either channel exceeds full scale. Track meters read after track effects and EQ. The master meter reads after master effects, EQ, and gain, before samples are clamped for output. External MIDI output has no audio return to meter.

The **Mixer** menu offers track starting points (Flat, Vocal presence, Bass weight, Acoustic sparkle) and master EQ/gain starting points (Flat unity, Warm tape EQ, Gentle smile EQ, Bright clarity). Select a track before applying a track preset. Tone and master settings apply during the session and are captured by GUI WAV export; Standard MIDI files retain their usual MIDI controls.

Track and mixer views show the instrument associated with each track’s initial bank and patch. On MIDI import, the app checks Program Change events before the first note, including events on a separate setup track. Patch numbers use the MIDI range (0–127), one lower than printed GM tables (1–128). With a loaded SoundFont, labels use its preset names and follow the software synth’s fallback for missing presets. Without a SoundFont, the app uses General MIDI names for bank 0, Drum Kit for channel 10, and Bank Preset for other banks.

## Virtual piano and track effects

With an SF2 loaded, open **Playback → Virtual piano** or press **Ctrl+K**. The window auditions the channel, bank, and patch of the track selected when you open it. Click a piano key or hold its printed computer key. The white keys are `qwertyuiop[]\`. Number keys `1–9` play the black keys from left to right. The left and right arrows move the keyboard by an octave. The chord indicator beside **Dock** shows the chord formed by the keys you hold, including notes held with the mouse.

In **Track Mixer**, drag the Reverb, Chorus, or Delay slider on an individual track. These effects process that track’s SoundFont audio before the master effects and are included in GUI WAV exports. The amounts are session settings.


## Recording and metronome

Select a track and press **Ctrl+R** or choose **Track → Record into selected track** to overdub from the playhead. The red Record button starts a separate take from the beginning. Select a MIDI input for hardware capture, or leave the selector on **MIDI input…** to open and record the virtual piano. Press **Stop** to keep the take. **Escape** also stops a take from the virtual piano window.

Toggle **Metronome** before or during playback or recording. Use the pattern selector to choose **Click**, **Rock**, **Four-on-floor**, **Hip-hop**, or **Shuffle**. The patterns follow the song tempo and repeat every four beats. On an empty project, enable Metronome and press Play to audition four bars, or enable Loop to keep them repeating. The click uses the selected playback output and stays out of MIDI saves and WAV exports. With the metronome enabled, recording provides up to one hour of click past the starting position.

## Count-in and harmony

Choose **Playback → Count-in** for zero, one, or two bars before recording. Choose **Count-in sound** for Click, Cowbell, or Side stick. The count uses the tempo at the entry point and four beats per bar. Recording starts after the count, and count-in notes stay out of saves and exports. Cowbell and Side stick are also available in the live metronome selector.

Click **Dock** on the virtual piano to hide it. The main window’s piano button shows whether the piano is docked during playback or recording. Click that button to bring the piano back. While docked, play from the main window with the same computer keys. The left and right arrows still shift the piano octave, and the transport shortcuts and text inputs keep their usual behavior. Existing hardware MIDI capture continues when the piano is docked.

The boxes beside the clock show the recognized chord, MIDI key signature at the playhead, and cumulative transposition in semitones since opening the file. Use the Key arrows to add or remove one sharp at the playhead, preserving major or minor mode. The Transpose arrows shift the song by one semitone. Chords come from overlapping notes on audible melodic tracks. Use **Edit → Set key signature** at the playhead to save a major or minor key change, or **Edit → Transpose song** to shift all melodic notes and key signatures by a signed number of semitones. General MIDI drums keep their original pitches.


## Scrollbars and chord ruler

Drag the bottom timeline scrollbar to browse beats in Track View and Piano Roll together. Use the right-side scrollbars or the mouse wheel over each pane to browse tracks or pitches independently. The Mixer has a separate horizontal scrollbar for channel strips. Starting playback or seeking on the beat ruler restores automatic playhead following after manual timeline scrolling.

The chord ruler above each beat ruler shows detected chord changes at their musical positions. A chord held into the visible range is shown at the left edge. The box beside the clock continues to show the chord at the current playhead.
