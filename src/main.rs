struct Date {
    name: String,
    age: u16,
}

fn main(){
let data = Date {
    name: String::from("Suisan"),
    age: 150,
};
if data.age <= 150{
    println!("Name: {}, Age: {}", data.name, data.age);
}else{
    println!("エラーです。年齢が150を超えています。");
}
}