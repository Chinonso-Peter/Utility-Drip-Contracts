use soroban_sdk::{Env, Address};

pub struct GasCostEstimator;

impl GasCostEstimator {
    // Gas costs for different operations (in stroops)
    const REGISTER_METER: i128 = 10_000_000; // 0.1 XLM
    const TOP_UP: i128 = 5_000_000; // 0.05 XLM
    const CLAIM: i128 = 8_000_000; // 0.08 XLM
    const UPDATE_HEARTBEAT: i128 = 3_000_000; // 0.03 XLM
    const GROUP_TOP_UP_PER_METER: i128 = 6_000_000; // 0.06 XLM per meter
    const EMERGENCY_SHUTDOWN: i128 = 2_000_000; // 0.02 XLM

    // Estimated monthly operations per meter
    const CLAIMS_PER_MONTH: u32 = 30; // Assuming daily claims
    const HEARTBEATS_PER_MONTH: u32 = 720; // Every hour for 30 days
    const TOP_UPS_PER_MONTH: u32 = 4; // Weekly top-ups

    pub fn estimate_meter_monthly_cost(
        _env: &Env,
        is_group_meter: bool,
        meters_in_group: u32,
    ) -> i128 {
        let mut monthly_cost = Self::REGISTER_METER; // One-time registration
        
        // Add recurring costs
        monthly_cost += (Self::CLAIM as u32 * Self::CLAIMS_PER_MONTH) as i128;
        monthly_cost += (Self::UPDATE_HEARTBEAT as u32 * Self::HEARTBEATS_PER_MONTH) as i128;
        monthly_cost += (Self::TOP_UP as u32 * Self::TOP_UPS_PER_MONTH) as i128;

        // For group meters, adjust top-up costs
        if is_group_meter {
            monthly_cost = monthly_cost - (Self::TOP_UP as u32 * Self::TOP_UPS_PER_MONTH) as i128;
            monthly_cost += (Self::GROUP_TOP_UP_PER_METER as u32 * Self::TOP_UPS_PER_MONTH) as i128;
        }

        monthly_cost
    }

    pub fn estimate_provider_monthly_cost(
        _env: &Env,
        number_of_meters: u32,
        percentage_group_meters: f32,
    ) -> i128 {
        let group_meters = (number_of_meters as f32 * percentage_group_meters) as u32;
        let individual_meters = number_of_meters - group_meters;

        let group_cost = if group_meters > 0 {
            // Assume average of 5 meters per group
            let groups = group_meters / 5;
            if groups > 0 {
                Self::estimate_meter_monthly_cost(_env, true, 5) * groups as i128
            } else {
                0
            }
        } else {
            0
        };

        let individual_cost = Self::estimate_meter_monthly_cost(_env, false, 0) * individual_meters as i128;

        group_cost + individual_cost
    }

    pub fn estimate_large_scale_cost(
        env: &Env,
        number_of_meters: u32,
        group_billing_enabled: bool,
    ) -> LargeScaleCostEstimate {
        let percentage_group = if group_billing_enabled { 0.8 } else { 0.0 }; // 80% in groups if enabled
        let monthly_cost = Self::estimate_provider_monthly_cost(env, number_of_meters, percentage_group);
        
        let annual_cost = monthly_cost * 12;
        let cost_per_meter = monthly_cost / number_of_meters as i128;
        
        // Convert to XLM (1 XLM = 10,000,000 stroops)
        let monthly_xlm = monthly_cost / 10_000_000;
        let annual_xlm = annual_cost / 10_000_000;
        let cost_per_meter_xlm = cost_per_meter / 10_000_000;

        LargeScaleCostEstimate {
            number_of_meters,
            monthly_cost_stroops: monthly_cost,
            annual_cost_stroops: annual_cost,
            cost_per_meter_stroops: cost_per_meter,
            monthly_cost_xlm,
            annual_cost_xlm,
            cost_per_meter_xlm,
            group_billing_enabled,
        }
    }

    pub fn get_operation_cost(operation: &str) -> i128 {
        match operation {
            "register_meter" => Self::REGISTER_METER,
            "top_up" => Self::TOP_UP,
            "claim" => Self::CLAIM,
            "update_heartbeat" => Self::UPDATE_HEARTBEAT,
            "group_top_up" => Self::GROUP_TOP_UP_PER_METER,
            "emergency_shutdown" => Self::EMERGENCY_SHUTDOWN,
            _ => 0,
        }
    }
}

#[contracttype]
#[derive(Clone)]
pub struct LargeScaleCostEstimate {
    pub number_of_meters: u32,
    pub monthly_cost_stroops: i128,
    pub annual_cost_stroops: i128,
    pub cost_per_meter_stroops: i128,
    pub monthly_cost_xlm: i128,
    pub annual_cost_xlm: i128,
    pub cost_per_meter_xlm: i128,
    pub group_billing_enabled: bool,
}

impl LargeScaleCostEstimate {
    pub fn get_summary(&self) -> String {
        format!(
            "Cost Analysis for {} meters:\n\
             Monthly: {} XLM ({} per meter)\n\
             Annual: {} XLM ({} per meter)\n\
             Group Billing: {}",
            self.number_of_meters,
            self.monthly_cost_xlm,
            self.cost_per_meter_xlm,
            self.annual_cost_xlm,
            self.cost_per_meter_xlm,
            if self.group_billing_enabled { "Enabled" } else { "Disabled" }
        )
    }
}
