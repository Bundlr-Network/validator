use diesel_async::deadpool::Pool;


pub type DbPool = Pool<Connec<PgConnection>>;

pub struct Validator {
    pub address: String,
    pub url: String
}