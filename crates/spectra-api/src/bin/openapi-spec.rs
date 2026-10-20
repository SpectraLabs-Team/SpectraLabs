//! Prints the engine API's OpenAPI description to stdout. `just api-client`
//! saves it and generates the dashboard's TypeScript types from it.

fn main() {
    println!("{}", spectra_api::openapi::spec_pretty_json());
}
