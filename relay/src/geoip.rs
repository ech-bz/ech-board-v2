use std::net::IpAddr;

use crate::error::RelayError;

pub struct GeoIp {
    reader: maxminddb::Reader<Vec<u8>>,
}

impl GeoIp {
    pub fn load(path: &str) -> Result<Self, RelayError> {
        let bytes = std::fs::read(path)
            .map_err(|error| RelayError::ConfigInvalid(format!("geoip {path}: {error}")))?;
        Ok(Self {
            reader: maxminddb::Reader::from_source(bytes)
                .map_err(|error| RelayError::ConfigInvalid(format!("geoip {path}: {error}")))?,
        })
    }

    pub fn country_code(&self, ip: IpAddr) -> Option<u32> {
        let result = self.reader.lookup(ip).ok()?;
        let country = result.decode::<maxminddb::geoip2::Country>().ok()??;
        let iso = country.country.iso_code?;
        iso3166::Country::from_alpha2(iso).map(|country| country.id as u32)
    }
}
