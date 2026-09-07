#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterKind {
    Characterwise,
    Linewise,
    Blockwise,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterValue {
    pub text: String,
    pub kind: RegisterKind,
}

#[derive(Debug, Clone, Default)]
pub struct RegisterBank {
    unnamed: Option<RegisterValue>,
}

impl RegisterBank {
    pub fn set(&mut self, text: String, kind: RegisterKind) {
        self.unnamed = Some(RegisterValue { text, kind });
    }

    pub fn unnamed(&self) -> Option<&RegisterValue> {
        self.unnamed.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{RegisterBank, RegisterKind};

    #[test]
    fn stores_the_unnamed_register_kind() {
        let mut registers = RegisterBank::default();
        registers.set("select".to_owned(), RegisterKind::Characterwise);

        let value = registers.unnamed().unwrap();
        assert_eq!(value.text, "select");
        assert_eq!(value.kind, RegisterKind::Characterwise);
    }
}
