# Threads and memory

The simulation, rendering and the console run on one thread: the one that
owns the window. Everything else the engine runs is a **module**, started and
stopped by `kerosene::lifecycle`.

## Start and stop

Modules start in three phases and stop in the opposite order:

1. **Engine**: the engine's own parts (today, the level-streaming workers).
2. **Modules**: the ones a game hands over in `Game::modules`.
3. **Game**: the game itself, through `Game::setup` and `Game::shutdown`.

Starting is two steps. Every module's `preload` runs, in order, before any
module's `start`, so a module can load what it needs while nothing is
running. `stop` runs in reverse. By the time a module's `stop` is called,
its threads have been told to stop and have been waited for. A thread that
ignores the request is logged and left behind after five seconds, so a stuck
worker can never hang the exit.

## A module of your own

```rust
use kerosene::lifecycle::{Ctx, Module};

struct Telemetry;

impl Module for Telemetry {
    fn name(&self) -> &'static str { "telemetry" }

    fn start(&mut self, ctx: &mut Ctx) -> kerosene::internals::kerror::Result<()> {
        ctx.spawn("flush", |token| {
            // `sleep` returns true as soon as a stop is asked for.
            while !token.sleep(std::time::Duration::from_secs(10)) {
                // ...send what has piled up...
            }
        })
    }
}
```

Return it from `Game::modules`. Spawn threads through the `Ctx`, never
`std::thread`: that is what names them (`kerosene-telemetry-flush`), counts
their memory under the module's name, and lets the manager stop them. A
panic on a managed thread is reported as a fatal `kengineThreadError` and
shuts the engine down cleanly. `ctx.pool("name", n)` gives a bounded pool of
workers; a job that panics is logged and does not take its worker with it.

The streaming pool has at most four workers. `KEROSENE_THREADS=n` sets the
count for everything that asks `default_workers()`.

## Memory

`TrackingAllocator` wraps the system allocator and counts. It is installed
by one line in the binary, because a library cannot choose the allocator;
new games have it already, and so does the stock `kerosene` binary:

```rust
#[global_allocator]
static ALLOCATOR: kerosene::lifecycle::TrackingAllocator =
    kerosene::lifecycle::TrackingAllocator::new();
```

The console command `mem` prints live and peak bytes, how many allocations
were made and freed, and a line per tag. A tag is a module's name for the
threads it owns, and `misc` for the main thread and everything else. `mem
reset_peak` starts the peak over. A free is counted under the tag of the
thread that frees, so a buffer handed between threads moves between tags and
a tag's figure can briefly be negative.

Without the line, `mem` says tracking is off and costs nothing: the system
allocator runs untouched. Delete the line from a release build if the few
atomic adds per allocation matter to you.
