// Copyright  (C) 2020, Hove and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
// the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
// powered by Hove (www.kisio.com).
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

extern crate static_assertions;

mod engine;
pub mod filters;
pub mod geometry;
pub mod models;
pub mod occupancy_data;
pub mod places_nearby;
pub mod request;
pub mod schedule;
pub mod time;
pub mod timetables;
pub mod transit_data;
pub mod transit_data_filtered;

pub use chrono::{self, NaiveDateTime};
pub use chrono_tz;
pub use time::PositiveDuration;
pub use tracing;
pub use transit_model;
pub use typed_index_collection;

pub use transit_data::data_interface::{
    Data as DataTrait, DataWithIters, RealTimeLevel, TransitTypes,
};

pub use occupancy_data::OccupancyData;

pub use transit_data::TransitData;

pub use engine::engine_interface::{
    BadRequest, InputStop, Request as RequestTrait, RequestDebug, RequestIO, RequestInput,
    RequestTypes, RequestWithIters,
};

pub use engine::multicriteria_raptor::MultiCriteriaRaptor;

pub mod response;

pub type Response = response::Response;

pub mod robustness;

#[macro_use]
extern crate lazy_static;
