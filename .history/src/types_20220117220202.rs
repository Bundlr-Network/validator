pub type DbPool = Pool<AsyncPgConnection>;

pub struct Validator {
    pub address: String,
    pub url: String
}