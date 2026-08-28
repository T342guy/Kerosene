# Sound

Three layers, separable on purpose.

| | |
|---|---|
| `void-audio::wav` | Files to samples |
| `void-audio::mixer` | Voices to a stereo buffer — no device, so it is testable |
| `void-audio::device` | That buffer to the sound card, behind a feature flag |

The split is the whole design. Everything that decides how a sound *sounds* —
falloff, panning, resampling, voice limits — is arithmetic on buffers with no
hardware in it, so a sound that pans the wrong way is a numeric fact rather
than something to notice by ear on the third playthrough.

**The mixer runs whether or not a sound card does.** If audio only existed when
a device opened, then everything about a game's behaviour that touches sound —
how many voices a trigger starts, whether a looping ambience got stopped —
would differ between a machine with sound and one without, and only one of
those would ever be tested. A missing device costs the last hop to the
speakers and nothing else.

## Playing something

```
play ui/click            # at the console
stopsound                # everything, now
snd_restart              # reopen the device, forget every decoded sound
volume 0.5               # master, archived
```

From a level: an `ambient_generic` entity, or a script.

```rhai
play_sound("ui/click");                                  // heard flat
play_sound("door/move", Vector(256.0, 256.0, 64.0));     // from a place
play_sound("door/move", find_by_name("gate").origin, 0.5);
stop_sounds();
```

## `.voidsnd` — sound scripts

A level fires `door/move`, and what that *is* lives in a script rather than on
the entity. The same indirection materials have, for the same reason: making
every door in a game quieter should be one edit, not a hunt through a map.

```
sound
{
    "name"        "door/move"
    "file"        "sound/door/move.wav"
    "volume"      "0.9"
    "pitch"       "1.0"
    "loop"        "0"
    "distance"    "128"     // full volume within this radius
    "attenuation" "1.0"     // how fast it falls off past it; 0 never does
    "max"         "2048"    // not heard at all past this
}
```

Every `.voidsnd` under `scripts/` loads at startup, later files overriding
earlier ones so a mod can change one sound without copying a file. A name
nothing defines is taken as a path under `sound/`, so `play ui/click.wav`
works before anyone has written a script.

## How a sound is heard

* **Inside `distance` it is at full volume.** Without that radius a sound at
  the listener's own position divides by zero, and one a step away is much
  quieter than one underfoot — neither of which is how hearing works.
* **Past it, inverse-distance falloff** scaled by `attenuation`. Zero carries
  forever, which is what music and a level-wide ambience want.
* **The last quarter of the range fades out**, so a sound does not audibly
  switch off as you step past its limit.
* **Panning is constant-power**: the two gains square-sum to one, so a sound
  crossing in front keeps the same loudness instead of dipping in the middle.
* **Gains ramp** rather than jumping. A discontinuity in a waveform is a click,
  and a sound moving past the listener changes gain every block.
* **64 voices at once**, and the quietest gives way. A trigger firing every
  tick would otherwise stack thousands of copies of the same sound, which is
  both deafening and slow.

## `ambient_generic`

The entity every level uses more than any other sound mechanism.

| Key | |
|---|---|
| `message` | Which sound |
| `health` | Volume, 0 to 1 — named as Source names it |
| `pitch` | Playback rate; 2 is an octave up |
| `radius` | How far it carries; 0 uses the script's |
| `looping` | Default on |
| Flag 1 | Starts silent, waits for `PlaySound` |
| Flag 2 | Heard everywhere — not positioned |

Inputs: `PlaySound`, `StopSound`, `Toggle`, `Volume`. Output: `OnPlay`.

## Formats

16-bit PCM WAV is the common case and what the sample content ships. Also
read: 8-bit (unsigned, centred on 128), 24-bit, 32-bit PCM, and 32-bit float,
mono or stereo, at any sample rate — anything not matching the device's rate is
resampled. `WAVE_FORMAT_EXTENSIBLE` headers are read through to the real
format, which is what a modern recorder writes.

The decoder is written out rather than pulled in, because a decoder is
somewhere an unexpected file should produce an error rather than a panic: a
chunk claiming more bytes than the file holds is normal, not an attack, and
takes what is there.

## Building without audio

`cpal` needs ALSA headers on Linux (`libasound2-dev`). If that is not
available:

```
cargo build --no-default-features -p void-runtime
```

Everything except the last hop to the speakers still builds, and the whole
mixer test suite still runs.
