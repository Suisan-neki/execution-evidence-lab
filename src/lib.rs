pub mod adapters;
pub mod ai;
mod ai_transport;
pub mod architecture;
pub mod assessment;
pub mod backup;
pub mod browser;
pub mod browser_evidence;
pub mod capture;
pub mod comparison;
pub mod container;
pub mod environment;
pub mod evaluation;
pub mod execution;
pub mod experiment;
pub mod http_probe;
pub mod model;
pub mod prepared_web;
pub mod profile;
pub mod protocol;
pub mod python_env;
pub mod reader;
pub mod recording;
pub mod reported;
pub mod review;
pub mod review_execution;
pub mod review_report;
pub mod server;
pub mod trial;
pub mod understanding;
pub mod workspace;

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
