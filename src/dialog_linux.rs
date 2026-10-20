//! "Open" dialog for choosing a playlist cover image on Linux, through rfd (already in the tree via
//! Freya): the desktop portal or GTK, whichever rfd was built with.

use std::path::PathBuf;

/// Blocks until the user picks a JPEG/PNG or cancels; `None` on cancel. Meant for a thread of its
/// own, so the UI keeps drawing meanwhile. The owner window is a Windows-only detail.
pub fn pick_image(_owner: Option<isize>) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Choose a playlist cover").add_filter("Images", &["jpg", "jpeg", "png"]).pick_file()
}
