//! One module per settings section. Each exposes `SEARCH`, `render`,
//! `modified` and `restore_defaults`; `SettingsPage` wires them in `Section`.

pub mod appearance;
pub mod archive;
pub mod connections;
pub mod general;
pub mod keybindings;
pub mod providers;
pub mod source_control;
pub mod storage;
