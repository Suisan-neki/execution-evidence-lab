struct Date {
    name: String,
    age: u16,
}

fn is_valid(data: &Date) -> bool {
    !data.name.is_empty() && data.age <= 150
}

fn main(){
let data = Date {
    name: String::from(""),
    age: 150,
};

println!("入力は有効？ {}", is_valid(&data));
}

#[cfg(test)]
mod tests {
    use super::{Date, is_valid};

    #[test]
    fn rejects_empty_name() {
        let data = Date {
            name: String::from(""),
            age: 30,
        };

        assert!(!is_valid(&data));
    }

    #[test]
    fn accepts_age_boundaries() {
        for age in [0, 150] {
            let data = Date {
                name: String::from("test"),
                age,
            };

            assert!(is_valid(&data), "age: {age}");
        }
    }

    #[test]
    fn rejects_ages_above_limit() {
        for age in [151, u16::MAX] {
            let data = Date {
                name: String::from("test"),
                age,
            };

            assert!(!is_valid(&data), "age: {age}");
        }
    }
}
