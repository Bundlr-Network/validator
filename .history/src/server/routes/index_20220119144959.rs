use actix_web::HttpResponse;

#[derive(Serialize)]
struct IndexBody {

}

pub async fn index() -> actix_web::Result<HttpResponse> {
    let body = 
    Ok(HttpResponse::Ok().finish())
}