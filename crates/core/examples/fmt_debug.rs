fn main() {
    println!("---{}---", shujing_core::sql_format::format_sql("select * from t where id in (1,2,3)"));
}
