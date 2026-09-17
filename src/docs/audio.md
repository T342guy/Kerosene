# Sound

Six layers, separable on purpose.

| | |
|---|---|
| `kerosene-audio::wav` | Source `.wav` files to samples |
| `kerosene-audio::compiled` | `.keroaud` files to samples — what a shipped game reads |
| `kerosene-audio::adpcm` | Four bits a sample, for the above |
| `kerosene-audio::mixer` | Voices to a stereo buffer — no device, so it is testable |
| `kerosene-audio::reverb`, `env`, `dsp` | The room and the air: what the world does to a sound on the way |
| `kerosene-audio::device` | That buffer to the sound card, behind a feature flag |

Sound has a compiled form as well as a source one. `timbre` turns `.wav` into
`.keroaud`, which is a quarter the size, carries loop points and a peak, and
records whether the sound may be positioned at all. The engine prefers it and
falls back to the `.wav` when there is no compiled form — so a designer who has
just dropped a file in hears it without running a build first, and a shipped
game carries only the small one. See [`tools.md`](tools.md#timbre--the-sound-compiler).

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
snd_reverb_preset hall   # hear a hall everywhere; empty to hear the map's rooms
snd_acoustics_debug 1    # say which room you are in as you move; 2 draws them
```

From a level: an `ambient_generic` entity, or a script.

```rhai
play_sound("ui/click");                                  // heard flat
play_sound("door/move", Vector(256.0, 256.0, 64.0));     // from a place
play_sound("door/move", find_by_name("gate").origin, 0.5);
stop_sounds();
```

## `.kerosnd` — sound scripts

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

Every `.kerosnd` under `scripts/` loads at startup, later files overriding
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

## How a room sounds

A sound in a level is heard through the level: an empty concrete hall rings,
a carpeted office is dead, a shout across a courtyard loses its consonants,
and a door between you and a radio turns it into a murmur. None of that is
placed. [Resonance](tools.md#resonance--the-acoustics-compiler) measures it
from the map's shape and materials at compile time, and the engine reads it
back every tick.

**The room.** The map carries one record per *room* — a set of leaves that
sound alike — saying how long sound lingers there in each of four bands, how
soon the first echo returns, how open to the sky it is, and how loud the room
is next to the sound itself. The mixer has one reverb, a feedback delay
network with eight lines and a per-band loss in each loop, and every tick the
engine hands it the record for the room the listener's ears are in. Only the
listener's room: a sound in the next room over is heard through *this* room,
which is how it works in a building. Near a doorway the two rooms' figures are
blended by distance, so stepping through one is a slide rather than a switch,
and the reverb slides its own parameters over a fifth of a second on top, so
nothing clicks.

**Air.** Every positioned sound is low-passed by its distance — 2 kHz at 4096
units, an octave lower for every 1233 beyond — because air really does eat
the highs, and a shout across a field should not arrive with every consonant
intact.

**Walls.** For every positioned sound the engine asks two questions. Is the
sound's cluster in the listener's *potentially audible set* — the PVS grown
by one room, which Umbra writes and this is the first thing to read? If not,
no path exists and the sound is silent. Otherwise, how many of three lines
from the ear to the sound — one straight, one to either side — are blocked?
A wall blocks all three and the sound comes through 12 dB down with nothing
above 800 Hz; a doorway lets one or two through and the sound comes round it
thinned rather than cut. Being in the same room halves the effect: a pillar
between two people in a hall is not a wall. Up to 24 sounds are re-traced
each tick, the rest keeping last tick's answer until their turn.

**What the compiler looked at.** Every surface's absorption per band comes
from its material — from `$surfaceprop`, or from an explicit `$acoustics`
key (see [`formats.md`](formats.md#keromat--materials)). A designer who
wants a room to sound a particular way regardless places an
`env_acoustic_override` in it.

| Convar | |
|---|---|
| `snd_reverb` | Room reverb; `0` is dry everywhere |
| `snd_reverb_preset` | `room`, `hall`, `cave` or `outdoor` everywhere, for auditioning; empty uses the map's |
| `snd_occlusion` | Walls muffle, and no-way-through silences |
| `snd_air` | Distance takes the highs out |
| `snd_acoustics_debug` | `1` reports the room as it changes; `2` also draws every room's leaves, blue for dead through red for live, and a line to each sound, green where it gets through |

Everything above is arithmetic on buffers with no device in it, like the rest
of the mixer. The reverb's decay times are checked in a test by measuring
them off its own impulse response; the compiler's are checked against
Eyring's formula for a concrete cube. A room that rings wrong is a number,
not a feeling.

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
cargo build --no-default-features -p kerosene-runtime
```

Everything except the last hop to the speakers still builds, and the whole
mixer test suite still runs.
