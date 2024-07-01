wit_bindgen::generate!();
use serde_json::Value;
use uuid::Uuid;
use exports::wasi::http::incoming_handler::Guest;
use wasi::http::types::*;
use serde::{ Deserialize, Serialize };
use anyhow::{ anyhow, bail, Result };
use crate::wasi::io::streams::StreamError;

struct HttpServer;

const MAX_READ_BYTES: u32 = 2048;

#[derive(Deserialize, Serialize, Clone, Debug)]
struct Todo {
    id: String,
    title: String,
}
impl Todo {
    fn new(id: String, title: String) -> Self {
        Todo { id, title }
    }
}

fn clean_string(input: &str) -> String {
    input.trim_matches('\\').trim_matches('\"').to_string()
}
impl Guest for HttpServer {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        // Permet de récupérer l'url
        let path_name = request.path_with_query().unwrap();
        // Séparation de l'url en suivant le symbole "/" dans un tableau
        let path_parts: Vec<&str> = path_name.split('/').collect();

        match (request.method(), path_parts.as_slice()) {
            (Method::Get, [_, "todos", ..]) => {
                let tasks = handle_get_tasks(); // représente le vecteur contenant les todos
                let response = OutgoingResponse::new(Fields::new());
                response.set_status_code(200).unwrap();
                let response_body = response.body().unwrap();
                let serialized_tasks = serde_json::to_vec(&tasks).unwrap();
                response_body.write().unwrap().blocking_write_and_flush(&serialized_tasks).unwrap();
                OutgoingBody::finish(response_body, None).expect("failed to finish response body");
                ResponseOutparam::set(response_out, Ok(response));
            }
            (Method::Post, [_, "todos", ..]) => {
                let param = request.read_body().unwrap(); // permet de récupérer le body de la requête en un Vec<u8>
                let result = std::str::from_utf8(&param).unwrap(); // tansforme le body en string
                let input: Value = serde_json::from_str(result).unwrap(); // formate le result en json
                let clean = clean_string(input["title"].to_string().as_str()); // élimine les "/" qui résultent du formatage en json
                // méthode pour faire les logs
                wasi::logging::logging::log(
                    wasi::logging::logging::Level::Info,
                    "",
                    &format!("sgdtht {:?}", clean)
                );
                add_tasks(Uuid::new_v4().to_string(), clean);
                let response = OutgoingResponse::new(Fields::new());
                response.set_status_code(200).unwrap();
                let response_body = response.body().unwrap();
                response_body.write().unwrap().blocking_write_and_flush(b"Task created").unwrap();
                OutgoingBody::finish(response_body, None).expect("failed to finish response body");
                ResponseOutparam::set(response_out, Ok(response));
            }
            (Method::Delete, [_, "todos", ..]) => {
                // L'id de l'élément à supprimer est récupéré dans le tableau des chemins "path_parts", à l'index 2
                delete(path_parts.get(2).unwrap().to_string());

                let response = OutgoingResponse::new(Fields::new());
                response.set_status_code(200).unwrap();
                let response_body = response.body().unwrap();
                response_body.write().unwrap().blocking_write_and_flush(b"Task deleted").unwrap();
                OutgoingBody::finish(response_body, None).expect("failed to finish response body");
                ResponseOutparam::set(response_out, Ok(response));
            }
            (Method::Put, [_, "todos", ..]) => {
                let param = request.read_body().unwrap();
                let result = std::str::from_utf8(&param).unwrap();
                let input: Value = serde_json::from_str(result).unwrap();
                let clean = clean_string(input["title"].to_string().as_str());

                // L'id de l'élément à modifier est récupéré dans le tableau des chemins "path_parts", à l'index 2
                update(path_parts.get(2).unwrap().to_string(), clean);

                let response = OutgoingResponse::new(Fields::new());
                response.set_status_code(200).unwrap();
                let response_body = response.body().unwrap();
                response_body.write().unwrap().blocking_write_and_flush(b"Task updated").unwrap();
                OutgoingBody::finish(response_body, None).expect("failed to finish response body");
                ResponseOutparam::set(response_out, Ok(response));
            }
            _ => {
                let response = OutgoingResponse::new(Fields::new());
                response.set_status_code(404).unwrap(); // Example: Not Found
                ResponseOutparam::set(response_out, Ok(response));
            }
        }
    }
}
export!(HttpServer);

// Fonction pour récupérer toutes les tasks de la base de donnée où elles sont stockées sous forme de key:value
fn handle_get_tasks() -> Vec<Todo> {
    // Récupération du bucket (redis)
    let bucket = wasi::keyvalue::store::open("").expect("failed to open empty bucket");
    // Récupération de la liste des keys de toutes les valeurs
    let binding = bucket.list_keys(None).unwrap();
    let count = binding.keys;
    let mut tasks = vec![];
    // Récupération de la valeur associée à chaque keys contenues dans le tableau
    for item in count {
        let value = bucket.get(&item).unwrap().unwrap();
        let result = std::str::from_utf8(&value).unwrap();
        let todo = Todo::new(item, result.to_string());
        tasks.push(todo);
    }
    tasks
}

fn add_tasks(id: String, value: String) {
    let bucket = wasi::keyvalue::store::open("").expect("failed to open empty bucket");
    // Ajout d'une nouvelle task (id, value) dans le store
    bucket.set(id.as_str(), value.as_bytes()).expect("failed to increment count");
}

fn delete(id: String) {
    let bucket = wasi::keyvalue::store::open("").expect("failed to open empty bucket");
    // Suppression d'une task grâçe à son id
    bucket.delete(&id).unwrap();
}

fn update(id: String, new_value: String) {
    let bucket = wasi::keyvalue::store::open("").expect("failed to open empty bucket");
    // Modification d'une task existante grâçe à son id
    bucket.set(&id, new_value.as_bytes()).unwrap();
}

impl IncomingRequest {
    /// This is a convenience function that writes out the body of a IncomingRequest (from wasi:http)
    /// into anything that supports [`std::io::Write`]
    fn read_body(self) -> Result<Vec<u8>> {
        // Read the body
        let incoming_req_body = self
            .consume()
            .map_err(|()| anyhow!("failed to consume incoming request body"))?;
        let incoming_req_body_stream = incoming_req_body
            .stream()
            .map_err(|()| anyhow!("failed to build stream for incoming request body"))?;
        let mut buf = Vec::<u8>::with_capacity(MAX_READ_BYTES as usize);
        loop {
            match incoming_req_body_stream.blocking_read(MAX_READ_BYTES as u64) {
                Ok(bytes) => buf.extend(bytes),
                Err(StreamError::Closed) => {
                    break;
                }
                Err(e) => bail!("failed to read bytes: {e}"),
            }
        }
        buf.shrink_to_fit();
        drop(incoming_req_body_stream);
        IncomingBody::finish(incoming_req_body);
        Ok(buf)
    }
}
