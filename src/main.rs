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