// A stand-in for Home Assistant: the three REST endpoints the example plugin
// uses, with the same paths, bodies and status codes, on a real loopback
// socket. LaunchDeck's broker is never mocked; this is the other end of the wire.
//
//   GET  /api/                              {"message": "API running."}
//   GET  /api/states/<entity_id>            the entity, or 404
//   POST /api/services/<domain>/<service>   turn_on, turn_off, toggle
//
// Every request needs `Authorization: Bearer <token>`; anything else is 401.
//
// Include `http_server.rs` before this file.

use std::collections::BTreeMap;

pub struct MockHomeAssistant {
    pub server: TestServer,
    entities: StdArc<StdMutex<BTreeMap<String, String>>>,
    delay: StdArc<StdMutex<StdDuration>>,
    pub token: String,
}

fn entity_json(id: &str, state: &str) -> String {
    format!(
        r#"{{"entity_id":"{id}","state":"{state}","attributes":{{"friendly_name":"{id}"}},"last_changed":"2026-10-04T19:00:00.000000+00:00","last_updated":"2026-10-04T19:00:00.000000+00:00"}}"#
    )
}

impl MockHomeAssistant {
    pub fn start(token: &str, entities: &[(&str, &str)]) -> MockHomeAssistant {
        let entities: StdArc<StdMutex<BTreeMap<String, String>>> = StdArc::new(StdMutex::new(
            entities.iter().map(|(id, state)| (id.to_string(), state.to_string())).collect(),
        ));
        let delay = StdArc::new(StdMutex::new(StdDuration::ZERO));
        let (table, wait, expected) = (entities.clone(), delay.clone(), format!("Bearer {token}"));
        let server = TestServer::start(move |request| {
            let wait = *wait.lock().unwrap();
            if request.header("authorization") != Some(expected.as_str()) {
                return Reply::text(401, "401: Unauthorized").after(wait);
            }
            let path = request.target.split('?').next().unwrap_or("");
            let reply = match (request.method.as_str(), path) {
                ("GET", "/api/") => Reply::json(200, r#"{"message":"API running."}"#),
                ("GET", p) if p.starts_with("/api/states/") => {
                    let id = &p["/api/states/".len()..];
                    match table.lock().unwrap().get(id) {
                        Some(state) => Reply::json(200, &entity_json(id, state)),
                        None => Reply::json(404, r#"{"message":"Entity not found."}"#),
                    }
                }
                ("POST", p) if p.starts_with("/api/services/") => {
                    let mut parts = p["/api/services/".len()..].split('/');
                    let (domain, service) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap_or(serde_json::Value::Null);
                    let Some(id) = body.get("entity_id").and_then(|v| v.as_str()) else {
                        return Reply::json(400, r#"{"message":"Data should be valid JSON."}"#).after(wait);
                    };
                    if !["turn_on", "turn_off", "toggle"].contains(&service) || !id.starts_with(&format!("{domain}.")) {
                        return Reply::json(400, r#"{"message":"Service not found."}"#).after(wait);
                    }
                    let mut table = table.lock().unwrap();
                    match table.get(id).cloned() {
                        // A service call that matches nothing changes nothing.
                        None => Reply::json(200, "[]"),
                        Some(now) => {
                            let next = match service {
                                "turn_on" => "on",
                                "turn_off" => "off",
                                _ if now == "on" => "off",
                                _ => "on",
                            };
                            // An entity that is unavailable stays so, as a device that is unplugged does.
                            if now != "unavailable" {
                                table.insert(id.to_string(), next.to_string());
                            }
                            Reply::json(200, &format!("[{}]", entity_json(id, table.get(id).unwrap())))
                        }
                    }
                }
                ("GET", _) | ("POST", _) => Reply::text(404, "404: Not Found"),
                _ => Reply::text(405, "405: Method Not Allowed"),
            };
            reply.after(wait)
        });
        MockHomeAssistant {
            server,
            entities,
            delay,
            token: token.to_string(),
        }
    }

    /// `http://127.0.0.1:<port>`: what goes in the plugin's address setting.
    pub fn origin(&self) -> String {
        self.server.origin()
    }

    /// Change an entity from "outside", as a wall switch or the Home Assistant app would.
    pub fn set(&self, entity: &str, state: &str) {
        self.entities.lock().unwrap().insert(entity.to_string(), state.to_string());
    }

    pub fn state(&self, entity: &str) -> Option<String> {
        self.entities.lock().unwrap().get(entity).cloned()
    }

    /// Make every answer take this long.
    pub fn slow(&self, by: StdDuration) {
        *self.delay.lock().unwrap() = by;
    }

    /// `domain/service entity` for every service call that arrived, in order.
    pub fn service_calls(&self) -> Vec<String> {
        self.server
            .received()
            .iter()
            .filter(|r| r.method == "POST" && r.target.starts_with("/api/services/"))
            .map(|r| {
                let body: serde_json::Value = serde_json::from_str(&r.body).unwrap_or(serde_json::Value::Null);
                format!("{} {}", &r.target["/api/services/".len()..], body["entity_id"].as_str().unwrap_or("?"))
            })
            .collect()
    }

    /// How many requests reached the server.
    pub fn hits(&self) -> usize {
        self.server.hits()
    }
}
