# Plan: Ratzilla Web Support for FinPlan TUI

This document outlines the plan to add a `web` feature flag that enables compiling the finplan TUI to WebAssembly using [Ratzilla](https://github.com/ratatui/ratzilla).

## Overview

Ratzilla provides web backends (DOM, Canvas, WebGL2) for ratatui applications, allowing the same widget code to run in browsers via WASM. The finplan TUI can be adapted with feature-flagged platform abstractions.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     finplan TUI                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Screens   │  │   Modals    │  │   Components        │  │
│  │  (ratatui)  │  │  (ratatui)  │  │   (ratatui)         │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         └────────────────┼────────────────────┘              │
│                          ▼                                   │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Platform Abstraction Layer                │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐  │  │
│  │  │  Terminal   │  │   Storage   │  │    Worker     │  │  │
│  │  │   Backend   │  │   Backend   │  │    Backend    │  │  │
│  │  └──────┬──────┘  └──────┬──────┘  └───────┬───────┘  │  │
│  └─────────┼────────────────┼─────────────────┼──────────┘  │
└────────────┼────────────────┼─────────────────┼─────────────┘
             │                │                 │
    ┌────────┴────────┐ ┌─────┴─────┐ ┌────────┴────────┐
    │  #[cfg(native)] │ │           │ │  #[cfg(native)] │
    │   crossterm     │ │           │ │  std::thread    │
    ├─────────────────┤ │           │ ├─────────────────┤
    │  #[cfg(web)]    │ │           │ │  #[cfg(web)]    │
    │   ratzilla      │ │           │ │  Web Worker     │
    └─────────────────┘ │           │ └─────────────────┘
                  ┌─────┴─────┐ ┌───┴────┐
                  │  native   │ │  web   │
                  │  std::fs  │ │ IndexedDB│
                  └───────────┘ └────────┘
```

## Compatibility Analysis

| Component | Native | Web (WASM) | Strategy |
|-----------|--------|------------|----------|
| ratatui widgets | crossterm | ratzilla | Feature flag backend |
| Event loop | poll-based | event-driven | Abstract event handling |
| File storage | std::fs | IndexedDB | Storage trait |
| Background work | std::thread | Web Worker | Worker trait |
| Logging | tracing-appender | console | Feature flag subscriber |
| Random | rand::rng() | getrandom | Enable WASM feature |
| Date/time | jiff | jiff | Already WASM-compatible |
| finplan_core | rayon parallel | sequential | Feature flag rayon |

## Implementation Phases

### Phase 1: Project Structure & Dependencies

**Goal:** Set up feature flags and dependencies without breaking native build.

#### 1.1 Update `crates/finplan/Cargo.toml`

```toml
[features]
default = ["native"]
native = ["crossterm", "dirs", "tracing-appender"]
web = ["ratzilla", "gloo-storage", "gloo-timers", "wasm-bindgen", "wasm-bindgen-futures", "web-sys", "console_error_panic_hook"]

[dependencies]
# Shared dependencies
finplan_core = { path = "../finplan_core" }
jiff = { workspace = true }
serde = { workspace = true }
serde-saphyr = "0.0.15"
rand = { version = "0.9", features = ["getrandom"] }
rand_distr = "0.5"
ratatui = { version = "0.30", default-features = false }
color-eyre = "0.6"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Native-only
crossterm = { version = "0.29", optional = true }
dirs = { version = "6.0", optional = true }
tracing-appender = { version = "0.2", optional = true }
clap = { version = "4.5", features = ["derive"], optional = true }

# Web-only
ratzilla = { version = "0.6", optional = true }
gloo-storage = { version = "0.3", optional = true }
gloo-timers = { version = "0.3", optional = true }
wasm-bindgen = { version = "0.2", optional = true }
wasm-bindgen-futures = { version = "0.4", optional = true }
web-sys = { version = "0.3", optional = true, features = ["console"] }
console_error_panic_hook = { version = "0.1", optional = true }
```

#### 1.2 Update `crates/finplan_core/Cargo.toml`

```toml
[features]
default = ["parallel"]
parallel = ["rayon"]

[dependencies]
rayon = { version = "1.11", optional = true }
```

#### 1.3 Create web build configuration

Create `crates/finplan/index.html`:
```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>FinPlan - Retirement Simulator</title>
    <style>
        body { margin: 0; background: #1a1a2e; }
        #terminal { width: 100vw; height: 100vh; }
    </style>
</head>
<body>
    <div id="terminal"></div>
</body>
</html>
```

Create `crates/finplan/Trunk.toml`:
```toml
[build]
target = "index.html"
dist = "dist"

[watch]
watch = ["src"]
```

---

### Phase 2: Platform Abstraction Layer

**Goal:** Create traits that abstract platform-specific functionality.

#### 2.1 Create `src/platform/mod.rs`

```rust
//! Platform abstraction layer for native/web compatibility

#[cfg(feature = "native")]
mod native;
#[cfg(feature = "web")]
mod web;

#[cfg(feature = "native")]
pub use native::*;
#[cfg(feature = "web")]
pub use web::*;

// Re-export platform-agnostic types
pub use storage::Storage;
pub use terminal::PlatformTerminal;
pub use worker::SimulationWorker;
```

#### 2.2 Create storage abstraction `src/platform/storage.rs`

```rust
use serde::{Deserialize, Serialize};
use std::error::Error;

pub trait Storage {
    fn list_scenarios(&self) -> Result<Vec<String>, Box<dyn Error>>;
    fn load_scenario(&self, name: &str) -> Result<ScenarioData, Box<dyn Error>>;
    fn save_scenario(&self, name: &str, data: &ScenarioData) -> Result<(), Box<dyn Error>>;
    fn delete_scenario(&self, name: &str) -> Result<(), Box<dyn Error>>;
    fn rename_scenario(&self, old: &str, new: &str) -> Result<(), Box<dyn Error>>;
}
```

#### 2.3 Create terminal abstraction `src/platform/terminal.rs`

```rust
use ratatui::Frame;
use std::io;

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
}

pub trait PlatformTerminal {
    fn run<F>(self, render: F) -> io::Result<()>
    where
        F: FnMut(&mut Frame) + 'static;

    fn on_event<F>(&mut self, handler: F)
    where
        F: FnMut(AppEvent) + 'static;
}
```

#### 2.4 Create worker abstraction `src/platform/worker.rs`

```rust
use finplan_core::{MonteCarloConfig, MonteCarloSummary, SimulationConfig};

pub trait SimulationWorker {
    fn start(&mut self, config: SimulationConfig, mc_config: MonteCarloConfig);
    fn cancel(&mut self);
    fn progress(&self) -> Option<(usize, usize)>; // (completed, total)
    fn result(&mut self) -> Option<MonteCarloSummary>;
    fn is_running(&self) -> bool;
}
```

---

### Phase 3: Native Platform Implementation

**Goal:** Wrap existing code in the platform abstraction.

#### 3.1 `src/platform/native/storage.rs`

Refactor existing `src/data/storage.rs` to implement the `Storage` trait using `std::fs`.

#### 3.2 `src/platform/native/terminal.rs`

Wrap existing crossterm/ratatui setup:
```rust
pub struct NativeTerminal {
    // Existing terminal setup
}

impl PlatformTerminal for NativeTerminal {
    fn run<F>(self, mut render: F) -> io::Result<()>
    where
        F: FnMut(&mut Frame) + 'static,
    {
        ratatui::run(|terminal| {
            // Existing event loop from app.rs
        })
    }
}
```

#### 3.3 `src/platform/native/worker.rs`

Refactor existing `src/worker.rs` to implement `SimulationWorker` using `std::thread`.

---

### Phase 4: Web Platform Implementation

**Goal:** Implement platform traits for WASM/browser environment.

#### 4.1 `src/platform/web/storage.rs`

```rust
use gloo_storage::{LocalStorage, Storage as GlooStorage};

pub struct WebStorage;

impl Storage for WebStorage {
    fn list_scenarios(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let index: Vec<String> = LocalStorage::get("finplan_scenarios")
            .unwrap_or_default();
        Ok(index)
    }

    fn save_scenario(&self, name: &str, data: &ScenarioData) -> Result<(), Box<dyn Error>> {
        let key = format!("finplan_scenario_{}", name);
        LocalStorage::set(&key, data)?;
        // Update index
        let mut index: Vec<String> = LocalStorage::get("finplan_scenarios")
            .unwrap_or_default();
        if !index.contains(&name.to_string()) {
            index.push(name.to_string());
            LocalStorage::set("finplan_scenarios", &index)?;
        }
        Ok(())
    }
    // ... other methods
}
```

#### 4.2 `src/platform/web/terminal.rs`

```rust
use ratzilla::{DomBackend, Terminal};
use ratzilla::event::KeyEvent;

pub struct WebTerminal {
    terminal: Terminal<DomBackend>,
}

impl WebTerminal {
    pub fn new() -> io::Result<Self> {
        let backend = DomBackend::new()?;
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl PlatformTerminal for WebTerminal {
    fn run<F>(mut self, render: F) -> io::Result<()>
    where
        F: FnMut(&mut Frame) + 'static,
    {
        self.terminal.draw_web(render);
        Ok(())
    }

    fn on_event<F>(&mut self, mut handler: F)
    where
        F: FnMut(AppEvent) + 'static,
    {
        self.terminal.on_key_event(move |event| {
            handler(AppEvent::Key(event));
        });
    }
}
```

#### 4.3 `src/platform/web/worker.rs`

For initial implementation, run simulations synchronously (blocking UI briefly):

```rust
pub struct WebWorker {
    result: Option<MonteCarloSummary>,
    running: bool,
}

impl SimulationWorker for WebWorker {
    fn start(&mut self, config: SimulationConfig, mc_config: MonteCarloConfig) {
        self.running = true;
        // Run synchronously for now (Phase 6 adds Web Workers)
        let result = finplan_core::monte_carlo_simulate(&config, &mc_config);
        self.result = Some(result);
        self.running = false;
    }
    // ...
}
```

---

### Phase 5: Refactor App to Use Platform Abstraction

**Goal:** Update `App` to use platform traits instead of direct dependencies.

#### 5.1 Update `src/app.rs`

```rust
use crate::platform::{PlatformTerminal, Storage, SimulationWorker};

pub struct App<T, S, W>
where
    T: PlatformTerminal,
    S: Storage,
    W: SimulationWorker,
{
    terminal: T,
    storage: S,
    worker: W,
    state: AppState,
}

impl<T, S, W> App<T, S, W>
where
    T: PlatformTerminal,
    S: Storage,
    W: SimulationWorker,
{
    pub fn new(terminal: T, storage: S, worker: W) -> Self {
        // ...
    }

    pub fn run(mut self) -> io::Result<()> {
        self.terminal.run(|frame| {
            self.render(frame);
        })
    }
}
```

#### 5.2 Update `src/main.rs` for native

```rust
#[cfg(feature = "native")]
fn main() -> color_eyre::Result<()> {
    use finplan::platform::native::{NativeTerminal, FileStorage, ThreadWorker};

    let terminal = NativeTerminal::new()?;
    let storage = FileStorage::new(data_dir);
    let worker = ThreadWorker::new();

    let app = App::new(terminal, storage, worker);
    app.run()?;
    Ok(())
}
```

#### 5.3 Create `src/lib.rs` for web entry point

```rust
#[cfg(feature = "web")]
use wasm_bindgen::prelude::*;

#[cfg(feature = "web")]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();

    use crate::platform::web::{WebTerminal, WebStorage, WebWorker};

    let terminal = WebTerminal::new().expect("Failed to create terminal");
    let storage = WebStorage::new();
    let worker = WebWorker::new();

    let app = App::new(terminal, storage, worker);
    app.run().expect("App failed");
}
```

---

### Phase 6: Web Worker for Background Simulations (Enhancement)

**Goal:** Run Monte Carlo simulations in a Web Worker to avoid blocking the UI.

#### 6.1 Create `src/platform/web/worker_thread.rs`

Use `wasm-bindgen` and `web-sys` to spawn a Web Worker:

```rust
use wasm_bindgen::prelude::*;
use web_sys::{Worker, MessageEvent};

pub struct WebWorkerThread {
    worker: Worker,
    result_receiver: Option<oneshot::Receiver<MonteCarloSummary>>,
}

impl WebWorkerThread {
    pub fn new() -> Self {
        let worker = Worker::new("./worker.js").expect("Failed to create worker");
        Self { worker, result_receiver: None }
    }
}
```

#### 6.2 Create worker entry point

Create `crates/finplan/worker.js`:
```javascript
import init, { run_simulation } from './finplan_worker.js';

self.onmessage = async (e) => {
    await init();
    const result = run_simulation(e.data.config, e.data.mc_config);
    self.postMessage(result);
};
```

---

### Phase 7: finplan_core WASM Compatibility

**Goal:** Ensure the simulation engine compiles to WASM.

#### 7.1 Feature-flag rayon in `crates/finplan_core/src/simulation.rs`

```rust
#[cfg(feature = "parallel")]
use rayon::prelude::*;

pub fn monte_carlo_simulate(config: &SimulationConfig, mc: &MonteCarloConfig) -> MonteCarloSummary {
    #[cfg(feature = "parallel")]
    let results: Vec<_> = (0..mc.iterations)
        .into_par_iter()
        .map(|i| simulate_single(config, seed + i))
        .collect();

    #[cfg(not(feature = "parallel"))]
    let results: Vec<_> = (0..mc.iterations)
        .map(|i| simulate_single(config, seed + i))
        .collect();

    // ...
}
```

#### 7.2 Verify jiff WASM compatibility

jiff should work in WASM. If issues arise, ensure the `js` feature is enabled:
```toml
jiff = { version = "0.2", features = ["js"] }
```

---

### Phase 8: Build & Deploy Infrastructure

#### 8.1 Add build scripts to `package.json` or `Makefile`

```makefile
.PHONY: build-web serve-web deploy

build-web:
	cd crates/finplan && trunk build --release

serve-web:
	cd crates/finplan && trunk serve

deploy:
	cd crates/finplan && trunk build --release
	# Deploy dist/ to static host
```

#### 8.2 GitHub Actions workflow

Create `.github/workflows/web.yml`:
```yaml
name: Build Web

on:
  push:
    branches: [main]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
      - name: Install trunk
        run: cargo install trunk
      - name: Build
        run: cd crates/finplan && trunk build --release
      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./crates/finplan/dist
```

---

## File Changes Summary

### New Files
| File | Purpose |
|------|---------|
| `crates/finplan/src/platform/mod.rs` | Platform abstraction module |
| `crates/finplan/src/platform/storage.rs` | Storage trait definition |
| `crates/finplan/src/platform/terminal.rs` | Terminal trait definition |
| `crates/finplan/src/platform/worker.rs` | Worker trait definition |
| `crates/finplan/src/platform/native/mod.rs` | Native implementations |
| `crates/finplan/src/platform/native/storage.rs` | File-based storage |
| `crates/finplan/src/platform/native/terminal.rs` | Crossterm terminal |
| `crates/finplan/src/platform/native/worker.rs` | Thread-based worker |
| `crates/finplan/src/platform/web/mod.rs` | Web implementations |
| `crates/finplan/src/platform/web/storage.rs` | LocalStorage/IndexedDB |
| `crates/finplan/src/platform/web/terminal.rs` | Ratzilla terminal |
| `crates/finplan/src/platform/web/worker.rs` | Web Worker simulation |
| `crates/finplan/src/lib.rs` | WASM entry point |
| `crates/finplan/index.html` | Web page template |
| `crates/finplan/Trunk.toml` | Trunk build config |
| `.github/workflows/web.yml` | CI/CD for web build |

### Modified Files
| File | Changes |
|------|---------|
| `crates/finplan/Cargo.toml` | Add feature flags, web dependencies |
| `crates/finplan_core/Cargo.toml` | Feature-flag rayon |
| `crates/finplan_core/src/simulation.rs` | Conditional parallel iteration |
| `crates/finplan/src/main.rs` | Platform-specific entry point |
| `crates/finplan/src/app.rs` | Use platform abstractions |
| `crates/finplan/src/data/storage.rs` | Extract to trait impl |
| `crates/finplan/src/worker.rs` | Extract to trait impl |
| `crates/finplan/src/logging.rs` | Feature-flag file appender |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Ratzilla API changes | Medium | High | Pin version, monitor releases |
| WASM bundle size too large | Medium | Medium | Enable LTO, strip, optimize |
| LocalStorage size limits (~5MB) | Low | Medium | Use IndexedDB for large scenarios |
| Web Worker complexity | High | Medium | Start with sync, add Workers later |
| Performance in browser | Medium | Medium | Profile, optimize hot paths |

---

## Success Criteria

1. `cargo build --features native` produces working native binary (existing behavior)
2. `trunk build --features web` produces working WASM bundle
3. All screens render correctly in browser
4. Scenarios can be saved/loaded in browser
5. Monte Carlo simulations complete in browser
6. Bundle size < 2MB gzipped

---

## Estimated Effort

| Phase | Effort |
|-------|--------|
| Phase 1: Project structure | Small |
| Phase 2: Platform traits | Small |
| Phase 3: Native implementation | Medium (refactoring) |
| Phase 4: Web implementation | Medium |
| Phase 5: App refactor | Large |
| Phase 6: Web Workers | Medium (optional) |
| Phase 7: Core WASM compat | Small |
| Phase 8: Build/deploy | Small |

---

## Next Steps

1. Review and approve this plan
2. Start with Phase 1 to set up feature flags
3. Implement phases incrementally, testing native build after each change
4. Test web build after Phase 4 is complete
