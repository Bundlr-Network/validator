use actix_web::{HttpResponse, web::Data};
use diesel::{QueryDsl, RunQueryDsl};
use diesel::prelude::ExpressionMethods;
use diesel_async::{deadpool::Pool, AsyncPgConnection};
use diesel_async::*;

use crate::database::models::Transaction;
use crate::{server::error::ValidatorServerError, types::DbPool, database::schema::transactions::dsl::*};


pub async fn get_tx(db: Data<Pool<AsyncPgConnection>>) -> actix_web::Result<HttpResponse, ValidatorServerError> {
    let mut conn = db.get()
        .await
        .unwrap();

    let res = transactions
        .filter(id.eq(""))
        .select(id)
        .limit(1)
        .get_result::<Transaction>(&mut conn)
        ;

    if let Ok(r) = res {
        Ok(HttpResponse::Ok().json(r))
    } else {
        Ok(HttpResponse::NotFound().finish())
    }
}