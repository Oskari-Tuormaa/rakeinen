pub mod kernel_editor;
pub mod matrix_editor;
pub mod palette_editor;
pub mod viewer;
pub mod wheel;

/// What an editor widget wants the app to do after this frame.
pub enum EditorAction {
    None,
    Edited,
    Save,
    Load,
}
