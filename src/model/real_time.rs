// Copyright  (C) 2020, Kisio Digital and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
// the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
// powered by Kisio Digital (www.kisio.com).
// Help us simplify mobility and open public transport:
// a non ending quest to the responsive locomotion way of traveling!
//
// This contribution is a part of the research and development work of the
// IVA Project which aims to enhance traveler information and is carried out
// under the leadership of the Technological Research Institute SystemX,
// with the partnership and support of the transport organization authority
// Ile-De-France Mobilités (IDFM), SNCF, and public funds
// under the scope of the French Program "Investissements d’Avenir".
//
// LICENCE: This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.
//
// Stay tuned using
// twitter @navitia
// channel `#navitia` on riot https://riot.im/app/#/room/#navitia:matrix.org
// https://groups.google.com/d/forum/navitia
// www.navitia.io

use std::{collections::HashMap, hash::Hash};

use tracing::error;

use crate::{
    chrono::NaiveDate,
    time::SecondsSinceTimezonedDayStart,
    timetables::FlowDirection,
    transit_data::{handle_insertion_errors, handle_removal_errors},
    transit_model::Model,
};

use crate::{DataUpdate, LoadsData};

use super::{ModelRefs, StopPointIdx, TransitModelVehicleJourneyIdx, VehicleJourneyIdx};

pub struct RealTimeModel {
    pub(super) new_vehicle_journeys_id_to_idx: HashMap<String, NewVehicleJourneyIdx>,
    // indexed by NewVehicleJourney.idx
    pub(super) new_vehicle_journeys_history: Vec<(String, VehicleJourneyHistory)>,

    // gives position in base_vehicle_journeys_history, if any
    pub(super) base_vehicle_journeys_idx_to_history: HashMap<TransitModelVehicleJourneyIdx, usize>,
    pub(super) base_vehicle_journeys_history: Vec<VehicleJourneyHistory>,

    pub(super) new_stop_id_to_idx: HashMap<String, NewStopPointIdx>,
    pub(super) new_stops: Vec<StopData>,
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct NewVehicleJourneyIdx {
    pub idx: usize, // position in new_vehicle_journeys_history
}

#[derive(Debug, Clone)]
pub struct VehicleJourneyHistory {
    by_reference_date: HashMap<NaiveDate, TripVersion>,
}
#[derive(Debug, Clone)]
pub struct TripVersion {
    disruption_id: String,
    trip_data: TripData,
}

#[derive(Debug, Clone)]
pub enum TripData {
    Deleted(),              // the trip is currently disabled
    Present(Vec<StopTime>), // list of all stop times of this trip
}

#[derive(Debug, Clone)]
pub struct StopTime {
    pub stop: StopPointIdx,
    pub arrival_time: SecondsSinceTimezonedDayStart,
    pub departure_time: SecondsSinceTimezonedDayStart,
    pub flow_direction: FlowDirection,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct NewStopPointIdx {
    pub idx: usize, // position in new_stops
}

pub struct StopData {
    pub(super) name: String,
}

#[derive(Debug, Clone)]
pub enum UpdateError {
    DeleteAbsentTrip(super::disruption::Trip),
    ModifyAbsentTrip(super::disruption::Trip),
    AddPresentTrip(super::disruption::Trip),
}

impl RealTimeModel {
    pub fn apply_disruption<Data: DataUpdate>(
        &mut self,
        disruption: &super::disruption::Disruption,
        model: &Model,
        loads_data: &LoadsData,
        real_time_data: &mut Data,
    ) {
        for update in disruption.updates.iter() {
            let apply_result =
                self.apply_update(&disruption.id, update, model, loads_data, real_time_data);
            if let Err(err) = apply_result {
                error!("Error occured while applying real time update {:?}", err);
            }
        }
    }

    pub fn apply_update<Data: DataUpdate>(
        &mut self,
        disruption_id: &str,
        update: &super::disruption::Update,
        model: &Model,
        loads_data: &LoadsData,
        data: &mut Data,
    ) -> Result<(), UpdateError> {
        match update {
            super::disruption::Update::Delete(trip) => {
                let vj_idx = self.delete(disruption_id, trip, model)?;
                let removal_result = data.remove_vehicle(&vj_idx, &trip.reference_date);
                if let Err(removal_error) = removal_result {
                    let model_ref = ModelRefs {
                        base: model,
                        real_time: self,
                    };
                    handle_removal_errors(
                        &model_ref,
                        data.start_date(),
                        data.end_date(),
                        std::iter::once(removal_error),
                    );
                }
                Ok(())
            }
            super::disruption::Update::Add(trip, stop_times) => {
                let (vj_idx, stop_times) = self.add(disruption_id, trip, stop_times, model)?;
                let dates = std::iter::once(&trip.reference_date);
                let stops = stop_times.iter().map(|stop_time| stop_time.stop.clone());
                let flows = stop_times.iter().map(|stop_time| stop_time.flow_direction);
                let board_times = stop_times.iter().map(|stop_time| stop_time.departure_time);
                let debark_times = stop_times.iter().map(|stop_time| stop_time.arrival_time);
                let insertion_errors = data.add_vehicle(
                    stops,
                    flows,
                    board_times,
                    debark_times,
                    loads_data,
                    dates,
                    &chrono_tz::UTC,
                    vj_idx,
                );
                let model_ref = ModelRefs {
                    base: model,
                    real_time: self,
                };
                handle_insertion_errors(
                    &model_ref,
                    data.start_date(),
                    data.end_date(),
                    &insertion_errors,
                );
                Ok(())
            }
            super::disruption::Update::Modify(trip, stop_times) => {
                let (vj_idx, stop_times) = self.modify(disruption_id, trip, stop_times, model)?;
                let dates = std::iter::once(&trip.reference_date);
                let stops = stop_times.iter().map(|stop_time| stop_time.stop.clone());
                let flows = stop_times.iter().map(|stop_time| stop_time.flow_direction);
                let board_times = stop_times.iter().map(|stop_time| stop_time.departure_time);
                let debark_times = stop_times.iter().map(|stop_time| stop_time.arrival_time);
                let (removal_errors, insertion_errors) = data.modify_vehicle(
                    stops,
                    flows,
                    board_times,
                    debark_times,
                    loads_data,
                    dates,
                    &chrono_tz::UTC,
                    vj_idx,
                );
                let model_ref = ModelRefs {
                    base: model,
                    real_time: self,
                };
                handle_insertion_errors(
                    &model_ref,
                    data.start_date(),
                    data.end_date(),
                    &insertion_errors,
                );
                handle_removal_errors(
                    &model_ref,
                    data.start_date(),
                    data.end_date(),
                    removal_errors.into_iter(),
                );
                Ok(())
            }
        }
    }

    pub fn delete(
        &mut self,
        disruption_id: &str,
        trip: &super::disruption::Trip,
        model: &Model,
    ) -> Result<VehicleJourneyIdx, UpdateError> {
        if self.is_present(trip, model) {
            let trip_version = TripVersion {
                disruption_id: disruption_id.to_string(),
                trip_data: TripData::Deleted(),
            };
            let idx = self.set_version(trip, model, trip_version);

            Ok(idx)
        } else {
            let err = UpdateError::DeleteAbsentTrip(trip.clone());
            Err(err)
        }
    }

    pub fn add(
        &mut self,
        disruption_id: &str,
        trip: &super::disruption::Trip,
        stop_times: &[super::disruption::StopTime],
        model: &Model,
    ) -> Result<(VehicleJourneyIdx, Vec<StopTime>), UpdateError> {
        if self.is_present(trip, model) {
            let err = UpdateError::AddPresentTrip(trip.clone());
            Err(err)
        } else {
            let stop_times = self.make_stop_times(stop_times, model);
            let trip_version = TripVersion {
                disruption_id: disruption_id.to_string(),
                trip_data: TripData::Present(stop_times.clone()),
            };
            let idx = self.set_version(trip, model, trip_version);
            Ok((idx, stop_times))
        }
    }

    pub fn modify(
        &mut self,
        disruption_id: &str,
        trip: &super::disruption::Trip,
        stop_times: &[super::disruption::StopTime],
        model: &Model,
    ) -> Result<(VehicleJourneyIdx, Vec<StopTime>), UpdateError> {
        if !self.is_present(trip, model) {
            let err = UpdateError::ModifyAbsentTrip(trip.clone());
            Err(err)
        } else {
            let stop_times = self.make_stop_times(stop_times, model);
            let trip_version = TripVersion {
                disruption_id: disruption_id.to_string(),
                trip_data: TripData::Present(stop_times.clone()),
            };
            let idx = self.set_version(trip, model, trip_version);
            Ok((idx, stop_times))
        }
    }

    fn is_present(&self, trip: &super::disruption::Trip, model: &Model) -> bool {
        if let Some(transit_model_idx) = model.vehicle_journeys.get_idx(&trip.vehicle_journey_id) {
            if let Some(&TripData::Deleted()) =
                self.base_vehicle_journey_last_version(&transit_model_idx, &trip.reference_date)
            {
                false
            } else {
                true
            }
        } else {
            self.new_vehicle_journeys_id_to_idx
                .contains_key(&trip.vehicle_journey_id)
        }
    }

    fn set_version(
        &mut self,
        trip: &super::disruption::Trip,
        model: &Model,
        trip_version: TripVersion,
    ) -> VehicleJourneyIdx {
        let (history, vj_idx) = if let Some(transit_model_idx) =
            model.vehicle_journeys.get_idx(&trip.vehicle_journey_id)
        {
            let histories = &mut self.base_vehicle_journeys_history;
            let idx = self
                .base_vehicle_journeys_idx_to_history
                .entry(transit_model_idx)
                .or_insert_with(|| {
                    let idx = histories.len();
                    histories.push(VehicleJourneyHistory::new());
                    idx
                });
            let history = &mut self.base_vehicle_journeys_history[*idx];
            let vj_idx = VehicleJourneyIdx::Base(transit_model_idx);
            (history, vj_idx)
        } else {
            let histories = &mut self.new_vehicle_journeys_history;
            let idx = self
                .new_vehicle_journeys_id_to_idx
                .entry(trip.vehicle_journey_id.clone())
                .or_insert_with(|| {
                    let idx = histories.len();
                    histories.push((
                        trip.vehicle_journey_id.clone(),
                        VehicleJourneyHistory::new(),
                    ));
                    NewVehicleJourneyIdx { idx }
                });
            let history = &mut self.new_vehicle_journeys_history[idx.idx].1;
            let vj_idx = VehicleJourneyIdx::New(idx.clone());
            (history, vj_idx)
        };

        history
            .by_reference_date
            .insert(trip.reference_date, trip_version);
        vj_idx
    }

    fn make_stop_times(
        &mut self,
        stop_times: &[super::disruption::StopTime],
        model: &Model,
    ) -> Vec<StopTime> {
        let mut result = Vec::new();
        for stop_time in stop_times {
            let stop_id = stop_time.stop_id.as_str();
            let stop_idx = self.get_or_insert_stop(stop_id, model);
            result.push(StopTime {
                stop: stop_idx,
                departure_time: stop_time.departure_time,
                arrival_time: stop_time.arrival_time,
                flow_direction: stop_time.flow_direction,
            });
        }
        result
    }

    fn get_or_insert_stop(&mut self, stop_id: &str, model: &Model) -> StopPointIdx {
        if let Some(idx) = model.stop_points.get_idx(stop_id) {
            StopPointIdx::Base(idx)
        } else if let Some(idx) = self.new_stop_id_to_idx.get(stop_id) {
            StopPointIdx::New(idx.clone())
        } else {
            let idx = NewStopPointIdx {
                idx: self.new_stops.len(),
            };
            self.new_stop_id_to_idx
                .insert(stop_id.to_string(), idx.clone());
            StopPointIdx::New(idx)
        }
    }

    pub(super) fn base_vehicle_journey_last_version(
        &self,
        idx: &TransitModelVehicleJourneyIdx,
        date: &NaiveDate,
    ) -> Option<&TripData> {
        self.base_vehicle_journeys_idx_to_history
            .get(idx)
            .map(|pos| {
                self.base_vehicle_journeys_history[*pos]
                    .by_reference_date
                    .get(date)
                    .map(|trip_version| &trip_version.trip_data)
            })
            .flatten()
    }

    pub(super) fn new_vehicle_journey_last_version(
        &self,
        idx: &NewVehicleJourneyIdx,
        date: &NaiveDate,
    ) -> Option<&TripData> {
        self.new_vehicle_journeys_history[idx.idx]
            .1
            .by_reference_date
            .get(date)
            .map(|trip_version| &trip_version.trip_data)
    }

    pub fn new() -> Self {
        Self {
            new_vehicle_journeys_id_to_idx: HashMap::new(),
            new_vehicle_journeys_history: Vec::new(),
            base_vehicle_journeys_idx_to_history: HashMap::new(),
            base_vehicle_journeys_history: Vec::new(),
            new_stop_id_to_idx: HashMap::new(),
            new_stops: Vec::new(),
        }
    }
}

impl Default for RealTimeModel {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for VehicleJourneyHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl VehicleJourneyHistory {
    pub fn new() -> Self {
        Self {
            by_reference_date: HashMap::new(),
        }
    }
}
