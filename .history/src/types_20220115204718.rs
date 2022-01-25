
pub type DbPool = Pool<ConnectionManager<PgConnection>>;

pub struct Validator {
    pub address: String,
    pub url: String
}