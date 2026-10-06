pub mod assessment;
pub mod capture;
pub mod experiment;
pub mod model;
pub mod protocol;
pub mod recording;

#[cfg(test)]
mod tests {
    use crate::model::{Data, is_valid};
    #[test]
    fn rejects_empty_name() {
        assert!(!is_valid(&Data {
            name: String::new(),
            age: 150
        }));
    }
    #[test]
    fn accepts_age_150() {
        assert!(is_valid(&Data {
            name: "架空の利用者".into(),
            age: 150
        }));
    }
    #[test]
    fn rejects_age_151() {
        assert!(!is_valid(&Data {
            name: "架空の利用者".into(),
            age: 151
        }));
    }
}
