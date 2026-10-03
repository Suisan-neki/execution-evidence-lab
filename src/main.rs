struct Date {
    name: String,
    age: u16,
}

fn main(){
let data = Date {
    name: String::from(""),
    age: 150,
};

if data.name.is_empty(){
    println!("名前が空です。");
}
else if data.age <= 150{
    println!("Name: {}, Age: {}", data.name, data.age);
}else{
    println!("エラーです。年齢が150を超えています。");
}
}