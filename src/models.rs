#[derive(Debug)]
pub enum BatteryStatus {
    Charged,
    Charging,
    Discharging,
    Unknown,
}

pub struct Battery {
    pub power: f32,
    pub energy: f32,
    pub capacity: f32,
    pub status: BatteryStatus,
}

impl Battery {
    pub fn new() -> Battery {
        Battery {
            power: 0.0,
            energy: 0.0,
            capacity: 0.0,
            status: BatteryStatus::Unknown,
        }
    }

    fn percent(&self) -> f32 {
        100.0 * self.energy / self.capacity
    }

    fn remaining(&self) -> f32 {
        // Watts / Watt*hrs
        self.energy / self.power
    }

    pub fn status(&self) -> String {
        match self.status {
            BatteryStatus::Charged => "charged".to_owned(),
            BatteryStatus::Charging => format!("{:.2}% (charging)", self.percent()),
            BatteryStatus::Discharging => {
                format!("{:.2}% ({:.2} hrs)", self.percent(), self.remaining())
            }
            BatteryStatus::Unknown => "no battery found".to_owned(),
        }
    }

    pub fn combine(&mut self, other: Battery) {
        self.power += other.power;
        self.capacity += other.capacity;
        self.energy += other.energy;

        match other.status {
            BatteryStatus::Charged => {
                // Implement std::cmp::PartialEq so I can just use an if statement.
                match self.status {
                    BatteryStatus::Charging => self.status = BatteryStatus::Charging,
                    BatteryStatus::Discharging => {}
                    _ => self.status = BatteryStatus::Charged,
                }
            }
            _ => self.status = other.status,
        };
    }
}
