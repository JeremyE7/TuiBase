use arboard::{Clipboard, Error};

pub fn copy_text(text: &str) -> Result<(), Error> {
    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(text.to_owned())
}

pub fn read_text() -> Result<String, Error> {
    let mut clipboard = Clipboard::new()?;
    clipboard.get_text()
}
