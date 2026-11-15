//! The Theme editor (prompt 086): the library of themes, the slide with its
//! direct-manipulation canvas, undo and redo, and saving. The inspector's
//! panels (type, layout, element, presentation) are prompt 087's.

pub mod backend;
pub mod canvas;
pub mod editor;
pub mod gallery;
pub mod history;
pub mod model;
pub mod ops;
pub mod view;

pub use backend::{MediaKind, Sample, ThemeBackend, ThemeFeed, ThemePreview};
pub use editor::{ThemeEditor, ThemeEditorEvent};
pub use view::ids;
