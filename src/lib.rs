//! WolOS shell: pure logic (tested) + GTK UI (`kit`, `ui`). Binaries in `src/bin/` are thin composition roots.
pub mod apps;
pub mod clipboard;
pub mod clock;
pub mod desktop;
pub mod hypr;
pub mod layout;
pub mod login;
pub mod overview;
pub mod perf;
pub mod protocol;
pub mod search;
pub mod setup;
pub mod status;
pub mod switcher;
pub mod system;
pub mod theme;

pub mod kit;
pub mod ui;
