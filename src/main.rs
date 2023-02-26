use actix_files as afs;
use actix_multipart::Multipart;
use actix_web::{get, post, App, HttpRequest, HttpResponse, HttpServer, Responder};
use futures_util::StreamExt as _;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

const CONTENT_DIR: &str = "./content";
const RETAIN_SECS: u64 = 1800;
const MAX_SIZE_IN_BYTES: usize = 1_000_000;

#[get("/")]
async fn hello(req: HttpRequest) -> impl Responder {
    let host: &str = match req.headers().get("Host") {
        Some(a) => a.to_str().unwrap_or("??"),
        None => "st.pepog.com",
    };
    let response = format!(
        "{host} Tmp Hosting\n\n
curl -F'file=@yourfile.png' https://{host}"
    );
    HttpResponse::Ok().body(response)
}

#[post("/")]
async fn upload(req: HttpRequest, mut payload: Multipart) -> impl Responder {
    let host: &str = match req.headers().get("Host") {
        Some(a) => a.to_str().unwrap_or("???"),
        None => "st.pepog.com",
    };

    let mut hasher = Sha256::new();
    let mut tempfile = NamedTempFile::new().unwrap();

    while let Some(item) = payload.next().await {
        let mut field = item.unwrap();

        let mut total_size = 0;
        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let chunk = &chunk.unwrap();
            total_size += chunk.len();
            if total_size > MAX_SIZE_IN_BYTES {
                return HttpResponse::Ok().body("too big\n");
            }
            hasher.update(chunk);
            tempfile.write(chunk).unwrap();
        }
        break; // I only want one
    }

    let result = hasher.finalize();
    let id = format!("{:x}", result).chars().take(6).collect::<String>();

    let (_file, path) = tempfile.keep().unwrap();

    let name = {
        let guess = infer::get_from_path(path.clone()).unwrap().unwrap();
        let ext = guess.extension();
        format!("{id}.{ext}")
    };

    let final_path = format!("{CONTENT_DIR}/{name}");
    fs::copy(path, final_path).unwrap();

    let response = format!("{host}/{name}\n");
    HttpResponse::Ok().body(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    thread::spawn(|| {
        loop {
            for entry in fs::read_dir(CONTENT_DIR).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if let Ok(time) = metadata.accessed() {
                    let now = std::time::SystemTime::now();
                    if let Ok(duration) = now.duration_since(time) {
                        if duration.as_secs() >= RETAIN_SECS {
                            fs::remove_file(entry.path()).unwrap();
                        }
                    } else {
                        // backwards time? PepoBan
                        fs::remove_file(entry.path()).unwrap();
                    }
                } else {
                    // No accessed time? PepoBan
                    fs::remove_file(entry.path()).unwrap();
                }
            }
            thread::sleep(Duration::from_secs(RETAIN_SECS.saturating_sub(3)));
        }
    });

    fs::create_dir_all(CONTENT_DIR).expect("Could not create \"host\" dir");
    HttpServer::new(|| {
        App::new().service(hello).service(upload).service(
            afs::Files::new("/", CONTENT_DIR)
                .show_files_listing()
                .use_last_modified(true),
        )
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
