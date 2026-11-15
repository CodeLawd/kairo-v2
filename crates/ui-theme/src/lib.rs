//! The Theme editor (prompt 086): the library of themes, the slide with its
//! direct-manipulation canvas, undo and redo, and saving. The inspector's
//! panels and the header's Screen Themes picker are prompt 087's.

pub mod backend;
pub mod canvas;
pub mod editor;
pub mod gallery;
pub mod history;
pub mod inspector;
pub mod model;
pub mod ops;
pub mod panels;
pub mod picker;
pub mod view;

pub use backend::{MediaKind, Sample, ThemeBackend, ThemeFeed, ThemePreview};
pub use editor::{ThemeEditor, ThemeEditorEvent};
pub use inspector::ThemeInspector;
pub use picker::ThemePicker;
pub use view::ids;
