use std::collections::HashMap;
use std::env;
use std::fmt::Display;
use std::thread::sleep;
use std::time::Duration;
use std::{fs, path::Path};
use base64::prelude::*;

use chrono::{DateTime, Local};
use reqwest::Error;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize, Deserialize, Debug)]
struct AuthResponse {
    access_token: String,
    scope: String,
    token_type: String,
    expires_in: u64
}

#[derive(Serialize, Deserialize, Debug)]
struct Location {
    gid: String
}
#[derive(Serialize, Deserialize, Debug)]
struct LocationResponse {
    results: Vec<Location>
}

#[derive(Serialize, Deserialize, Debug)]
struct LineResponse {
    name: String,
    designation: String
}
#[derive(Serialize, Deserialize, Debug)]
struct ServiceJourney {
    direction: String,
    line: LineResponse
}
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
struct DepartureResponse {
    serviceJourney: ServiceJourney,
    estimatedOtherwisePlannedTime: DateTime<chrono::FixedOffset>,
    isCancelled: bool,
    isPartCancelled: bool
}
#[derive(Serialize, Deserialize, Debug)]
struct DepartureCallResponse {
    results: Vec<DepartureResponse>
}

impl Display for DepartureResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.serviceJourney.line.designation, self.serviceJourney.direction, self.estimatedOtherwisePlannedTime)
    }
}

#[derive(PartialEq, Eq, Hash)]
struct Line {
    line_num: String,
    target: String
}
impl Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.line_num, self.target)
    }
}
#[derive(PartialEq, Eq, Hash)]
struct Departure {
    time: DateTime<chrono::FixedOffset>,
    is_canelled: bool,
    is_part_cancelled: bool
}
impl Display for Departure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dt: i64 = (self.time - Local::now().fixed_offset()).num_minutes();
        match (self.is_canelled, self.is_part_cancelled) {
            (true, _) => write!(f, "Cancelled"),
            (false, true) => write!(f, "*{}", dt),
            (false, false) => write!(f, "{}", dt),
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let stop_name: &str = &args[1];
    let platforms: &str = &args[2];
    let client: Client = Client::new();
    let (mut auth_token, mut auth_expiry): (String, DateTime<Local>) = authenticate(&client).unwrap();
    let base_url: &str = "https://ext-api.vasttrafik.se/pr/v4";
    let gid: &str = &get_location_gid(&client, &auth_token, &base_url, &stop_name).unwrap();

    loop {
        if auth_expiry < Local::now() {
            (auth_token, auth_expiry) = authenticate(&client).unwrap();
        }
        let departure_responses: Vec<DepartureResponse> = get_departures(&client, &auth_token, &base_url, &gid, &platforms).unwrap();
        let departure_map: HashMap<Line, Vec<Departure>> = merge_departures(departure_responses);
        for (line, departures) in departure_map.into_iter() {
            println!("{}: {}", line, departures.iter().map(| d:&Departure | d.to_string()).collect::<Vec<String>>().join(" "))
        }
        sleep(Duration::from_secs(30));
    }
}

fn merge_departures(departure_responses: Vec<DepartureResponse>) -> HashMap<Line, Vec<Departure>> {
    let mut departures: HashMap<Line, Vec<Departure>> = HashMap::new();
    for dep_res in departure_responses {
        let line: Line = Line{
            line_num: dep_res.serviceJourney.line.designation,
            target: dep_res.serviceJourney.direction
        };
        let departure: Departure = Departure{
            time: dep_res.estimatedOtherwisePlannedTime,
            is_canelled: dep_res.isCancelled,
            is_part_cancelled: dep_res.isPartCancelled
        };
        departures.entry(line).or_insert(Vec::new()).push(departure);
    }
    return departures;
}

fn get_departures(client: &Client, auth_token: &String, base_url: &&str, gid: &&str, platforms: &&str) -> Result<Vec<DepartureResponse>, Error> {
    let response: Result<Response, Error> = client.get(format!("{base_url}/stop-areas/{gid}/departures?platforms={platforms}&maxDeparturesPerLineAndDirection=2&limit=10&offset=0&includeOccupancy=false"))
        .header("accept", "text/plain")
        .header("Authorization", format!("Bearer {}", auth_token))
        .send();
    return match response {
        Ok(res) => {
            let parsed_res: DepartureCallResponse = serde_json::from_str(&res.text().unwrap()).unwrap();
            Ok(parsed_res.results)
        },
        Err(e) => Err(e),
    }
}

fn get_location_gid(client: &Client, auth_token: &String, base_url: &&str, stop_name: &&str) -> Result<String, Error> {
    let response: Result<Response, Error> = client.get(format!("{base_url}/locations/by-text?q={stop_name}&types=stoparea&limit=1&offset=0"))
        .header("accept", "text/plain")
        .header("Authorization", format!("Bearer {}", auth_token))
        .send();
    return match response {
        Ok(res) => {
            let response_struct: LocationResponse = serde_json::from_str(&res.text().unwrap()).unwrap();
            Ok(response_struct.results.first().unwrap().gid.clone())
        },
        Err(e) => Err(e),
    }

}

fn authenticate(client: &Client) -> Result<(String, DateTime<Local>), Error> {
    let client_id: String = fs::read_to_string(Path::new("credentials/client_id")).unwrap();
    let client_secret: String = fs::read_to_string(Path::new("credentials/client_secret")).unwrap();
    let authentication_key: String = BASE64_STANDARD.encode(format!("{client_id}:{client_secret}"));

    let response: Result<Response, Error>  = client.post("https://ext-api.vasttrafik.se/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Authorization", format!("Basic {authentication_key}"))
        .body("grant_type=client_credentials")
        .send();

    return match response {
        Ok(res) => {
            let auth_response: AuthResponse = serde_json::from_str::<AuthResponse>(&res.text().unwrap()).unwrap();
            let auth_expiry: DateTime<Local> = Local::now() + Duration::from_secs(auth_response.expires_in - 60);
            Ok((auth_response.access_token, auth_expiry))
        },
        Err(e) => Err(e),
    }
}
