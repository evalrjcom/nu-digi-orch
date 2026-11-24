# Nu Digi Orch

Nu Digi Orch is a lightweight desktop MIDI composer in Rust and FLTK. It combines a track list and piano roll with SoundFont playback, hardware MIDI routing, and offline WAV rendering.

## Track view and playhead

The composer opens in split view. The upper table combines track names, channel, program, bank, volume, pan, mute, and solo with the arrangement lanes. Click a row to edit that track in the piano roll below. Double-click a name or numeric cell to edit it, or click M or S to toggle mute or solo. Scroll over the table to reach additional tracks.

Choose **Full track view** for the arrangement alone. Click a beat on either ruler to cue playback or jump while playing. Play and Loop start at the cue, and Stop resets it. Use the tempo +/− buttons or click the up/down controls beside channel, program, bank, volume, or pan. Select a numeric cell to adjust it with the keyboard arrow keys.

Choose **Full piano roll** or double-click an arrangement lane to expand the piano roll across the editing area. **Split view** restores the table. Both panes share the same beat grid and playback position.

During playback and recording, the amber vertical line marks the current musical position in both views. It follows tempo changes, advances to the next 16-beat page, and returns to the beginning during loop playback.

## Start composing

1. Open a MIDI file with **File → Open MIDI**, or open the built-in two-track song with **File → Open Demo**.
2. Choose **Load SF2** and select a SoundFont 2 file. The software synth plays through the system default or a device selected in **Audio device**. On Linux the app also checks common system SoundFont locations. Set `NU_DIGI_ORCH_SF2` to load a specific file at startup.
3. Press **Play** or activate **Loop** before playback. To send MIDI to hardware or another application, select its port in **Output**.
4. Double-click a track lane to open its piano roll. Click the piano roll to add or select a note. Drag a note to move it in pitch and time. Right-click or press Delete with the roll focused to remove a note. The note fields below the roll set exact tick position, pitch, duration, and velocity. **Snap** controls the edit grid, and **New length** controls the duration of newly drawn notes.
5. Add tracks with **+**. Select a MIDI channel and program and set bank, volume, and pan in the track table. Channel 10 is percussion in General MIDI. With an SF2 loaded, you can also choose a named preset. Tracks sharing a MIDI channel share its patch and controllers. Mute and Solo control playback and WAV export for the current session.
6. Pick a port in **MIDI input…**, then press **Record MIDI**. The app captures a take from the start of the timeline. Play or monitor the instrument on your MIDI device during capture. **Stop** adds the take as a new editable track.
7. Save a standard MIDI file, or use **File → Export WAV** to mix the audible tracks to a stereo 44.1 kHz, 16-bit WAV with the loaded SoundFont.

For longer songs, set **First beat** to move horizontally and **Top key** to change the pitch range. Use Ctrl+Z and Ctrl+Y for undo and redo.

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
