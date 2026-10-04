struct Date {
    name: String,
    age: u16,
}

fn is_valid(data: &Date) -> bool {
    if data.name.is_empty(){
        return false;
    }
    else if data.age <= 150{
        return true;
    }
    else{
        return false;
    }
}

fn main(){
let data = Date {
    name: String::from(""),
    age: 150,
};

println!("入力は有効？ {}", is_valid(&data));
}