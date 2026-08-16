use anyhow::{bail, Result};

pub const EPCIS_EVENT_VERSION: u8 = 1;
pub const EPCIS_EVENT_V1_BYTES: usize = 86;
pub const MAX_POLYGON_VERTICES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointE6 {
    pub latitude_e6: i32,
    pub longitude_e6: i32,
}

impl PointE6 {
    pub const fn new(latitude_e6: i32, longitude_e6: i32) -> Self {
        Self {
            latitude_e6,
            longitude_e6,
        }
    }

    pub fn validate(self) -> Result<()> {
        if !(-90_000_000..=90_000_000).contains(&self.latitude_e6) {
            bail!("latitude is outside WGS-84 microdegree bounds");
        }
        if !(-180_000_000..=180_000_000).contains(&self.longitude_e6) {
            bail!("longitude is outside WGS-84 microdegree bounds");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimplePolygon {
    vertices: Vec<PointE6>,
}

impl SimplePolygon {
    pub fn new(vertices: Vec<PointE6>) -> Result<Self> {
        if !(3..=MAX_POLYGON_VERTICES).contains(&vertices.len()) {
            bail!("polygon must have 3 to {MAX_POLYGON_VERTICES} vertices");
        }
        for point in &vertices {
            point.validate()?;
        }
        for i in 0..vertices.len() {
            if vertices[i] == vertices[(i + 1) % vertices.len()] {
                bail!("polygon has duplicate adjacent vertices");
            }
        }
        for i in 0..vertices.len() {
            for j in (i + 1)..vertices.len() {
                if are_adjacent(i, j, vertices.len()) {
                    continue;
                }
                if segments_intersect(
                    vertices[i],
                    vertices[(i + 1) % vertices.len()],
                    vertices[j],
                    vertices[(j + 1) % vertices.len()],
                ) {
                    bail!("polygon self-intersects");
                }
            }
        }
        Ok(Self { vertices })
    }

    pub fn vertices(&self) -> &[PointE6] {
        &self.vertices
    }

    pub fn contains_strict(&self, point: PointE6) -> bool {
        if self.vertices.iter().enumerate().any(|(index, start)| {
            point_on_segment(
                *start,
                self.vertices[(index + 1) % self.vertices.len()],
                point,
            )
        }) {
            return false;
        }

        let mut inside = false;
        for index in 0..self.vertices.len() {
            let start = self.vertices[index];
            let end = self.vertices[(index + 1) % self.vertices.len()];
            if (start.latitude_e6 > point.latitude_e6) != (end.latitude_e6 > point.latitude_e6) {
                let side = orientation(start, end, point);
                let crosses_right = if end.latitude_e6 > start.latitude_e6 {
                    side > 0
                } else {
                    side < 0
                };
                if crosses_right {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpcisEventV1 {
    pub event_id: u64,
    pub lot_id: u64,
    pub epoch_id: u64,
    pub timestamp_ms: u64,
    pub readings: u32,
    pub latitude_e6: i32,
    pub longitude_e6: i32,
    pub certificate_id: u64,
    pub role: u8,
    pub actor_public_key: [u8; 32],
}

impl EpcisEventV1 {
    pub fn point(&self) -> PointE6 {
        PointE6::new(self.latitude_e6, self.longitude_e6)
    }

    pub fn validate(&self) -> Result<()> {
        self.point().validate()
    }

    pub fn canonical_bytes(&self) -> [u8; EPCIS_EVENT_V1_BYTES] {
        let mut output = [0u8; EPCIS_EVENT_V1_BYTES];
        output[0] = EPCIS_EVENT_VERSION;
        output[1..9].copy_from_slice(&self.event_id.to_be_bytes());
        output[9..17].copy_from_slice(&self.lot_id.to_be_bytes());
        output[17..25].copy_from_slice(&self.epoch_id.to_be_bytes());
        output[25..33].copy_from_slice(&self.timestamp_ms.to_be_bytes());
        output[33..37].copy_from_slice(&self.readings.to_be_bytes());
        output[37..41].copy_from_slice(&self.latitude_e6.to_be_bytes());
        output[41..45].copy_from_slice(&self.longitude_e6.to_be_bytes());
        output[45..53].copy_from_slice(&self.certificate_id.to_be_bytes());
        output[53] = self.role;
        output[54..86].copy_from_slice(&self.actor_public_key);
        output
    }

    pub fn decode_canonical(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != EPCIS_EVENT_V1_BYTES {
            bail!("EPCIS event must be exactly {EPCIS_EVENT_V1_BYTES} bytes");
        }
        if bytes[0] != EPCIS_EVENT_VERSION {
            bail!("unsupported EPCIS event version");
        }
        let event = Self {
            event_id: read_u64(bytes, 1),
            lot_id: read_u64(bytes, 9),
            epoch_id: read_u64(bytes, 17),
            timestamp_ms: read_u64(bytes, 25),
            readings: read_u32(bytes, 33),
            latitude_e6: read_i32(bytes, 37),
            longitude_e6: read_i32(bytes, 41),
            certificate_id: read_u64(bytes, 45),
            role: bytes[53],
            actor_public_key: bytes[54..86]
                .try_into()
                .expect("canonical EPCIS payload has a fixed public-key width"),
        };
        event.validate()?;
        Ok(event)
    }
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    bytes[offset..offset + 8]
        .try_into()
        .map(u64::from_be_bytes)
        .expect("canonical EPCIS payload has fixed u64 fields")
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    bytes[offset..offset + 4]
        .try_into()
        .map(u32::from_be_bytes)
        .expect("canonical EPCIS payload has fixed u32 fields")
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    bytes[offset..offset + 4]
        .try_into()
        .map(i32::from_be_bytes)
        .expect("canonical EPCIS payload has fixed i32 fields")
}

fn are_adjacent(first: usize, second: usize, len: usize) -> bool {
    first + 1 == second || (first == 0 && second + 1 == len)
}

fn orientation(start: PointE6, end: PointE6, point: PointE6) -> i128 {
    (end.longitude_e6 as i128 - start.longitude_e6 as i128)
        * (point.latitude_e6 as i128 - start.latitude_e6 as i128)
        - (end.latitude_e6 as i128 - start.latitude_e6 as i128)
            * (point.longitude_e6 as i128 - start.longitude_e6 as i128)
}

fn point_on_segment(start: PointE6, end: PointE6, point: PointE6) -> bool {
    orientation(start, end, point) == 0
        && point.longitude_e6 >= start.longitude_e6.min(end.longitude_e6)
        && point.longitude_e6 <= start.longitude_e6.max(end.longitude_e6)
        && point.latitude_e6 >= start.latitude_e6.min(end.latitude_e6)
        && point.latitude_e6 <= start.latitude_e6.max(end.latitude_e6)
}

fn segments_intersect(a: PointE6, b: PointE6, c: PointE6, d: PointE6) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);

    (ab_c == 0 && point_on_segment(a, b, c))
        || (ab_d == 0 && point_on_segment(a, b, d))
        || (cd_a == 0 && point_on_segment(c, d, a))
        || (cd_b == 0 && point_on_segment(c, d, b))
        || ((ab_c > 0) != (ab_d > 0) && (cd_a > 0) != (cd_b > 0))
}
