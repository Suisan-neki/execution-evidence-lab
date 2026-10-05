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

#[test]
fn rejects_empty_name() {
    let data = Date {
        name: String::from(""),
        age: 30,
    };

    assert!(!is_valid(&data));
}

#[test]
fn accepts_age_150() {
    let data = Date {
        name: String::from("テスト"),
        age: 150,
    };

    assert!(is_valid(&data));
}
