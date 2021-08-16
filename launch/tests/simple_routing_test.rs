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

mod utils;
use failure::Error;
use launch::datetime::DateTimeRepresent;
use loki::modelbuilder::ModelBuilder;
use loki::PeriodicData;
use utils::{build_and_solve, dt_from_str, make_pt_from_vehicle, make_stop_point, Config};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[test]
fn test_simple_routing() -> Result<(), Error> {
    init();

    let model = ModelBuilder::new("20200101", "20200102")?
        .calendar("service1", &["2020-01-01"])
        .route("1", |r| {
            r.name = String::from("bob");
        })
        .vj("toto", |vj_builder| {
            vj_builder
                .calendar("service1")
                .route("1")
                .st("A", "10:00:00", "10:00:01")
                .st("B", "10:05:00", "10:05:01")
                .st("C", "10:10:00", "10:10:01");
        })
        .build();

    let config = Config::new("20200101T085900", "A", "B");

    let responses = build_and_solve::<PeriodicData>(&model, &loki::LoadsData::empty(), &config)?;

    assert_eq!(model.vehicle_journeys.len(), 1);
    assert_eq!(responses.len(), 1);

    let journey = &responses[0];
    assert_eq!(journey.nb_of_sections(), 1);
    assert_eq!(journey.connections.len(), 0);

    // First Vehicle
    let vehicle_sec = &journey.first_vehicle;
    assert_eq!(journey.first_vj_uri(&model), "toto");
    let (from_sp, to_sp) = make_pt_from_vehicle(vehicle_sec, &model)?;
    assert_eq!(from_sp.name, "A");
    assert_eq!(to_sp.name, "B");
    assert_eq!(vehicle_sec.from_datetime, dt_from_str("20200101T090001")?);
    assert_eq!(vehicle_sec.to_datetime, dt_from_str("20200101T090500")?);

    assert_eq!(journey.nb_of_transfers(), 0);
    assert_eq!(journey.total_duration(), 360);

    Ok(())
}

#[test]
fn test_routing_with_transfers() -> Result<(), Error> {
    init();

    let model = ModelBuilder::new("20200101", "20200102")?
        .calendar("service1", &["2020-01-01"])
        .vj("toto", |vj_builder| {
            vj_builder
                .calendar("service1")
                .st("A", "10:00:00", "10:00:01")
                .st("B", "10:05:00", "10:05:01")
                .st("C", "10:10:00", "10:10:01");
        })
        .vj("tata", |vj_builder| {
            vj_builder
                .calendar("service1")
                .st("E", "10:05:00", "10:05:01")
                .st("F", "10:20:00", "10:20:01")
                .st("G", "10:30:00", "10:30:01");
        })
        .add_transfer("B".into(), "F".into(), 120)
        .build();

    let config = Config::new("20200101T085900", "A", "G");

    let responses = build_and_solve::<PeriodicData>(&model, &loki::LoadsData::empty(), &config)?;

    assert_eq!(model.vehicle_journeys.len(), 2);
    assert_eq!(responses.len(), 1);

    let journey = &responses[0];
    assert_eq!(journey.first_vj_uri(&model), "toto");
    assert_eq!(journey.nb_of_sections(), 4);

    assert_eq!(journey.nb_of_transfers(), 1);
    assert_eq!(journey.total_duration(), 1860);

    // First Vehicle
    let vehicle_sec = &journey.first_vehicle;
    let (from_sp, to_sp) = make_pt_from_vehicle(vehicle_sec, &model)?;
    assert_eq!(from_sp.name, "A");
    assert_eq!(to_sp.name, "B");
    assert_eq!(vehicle_sec.from_datetime, dt_from_str("20200101T090001")?);
    assert_eq!(vehicle_sec.to_datetime, dt_from_str("20200101T090500")?);

    // Transfer section
    assert_eq!(journey.connections.len(), 1);
    let transfer_sec = &journey.connections[0].0;
    let start_transfer_sp = make_stop_point(&transfer_sec.from_stop_point, &model);
    assert_eq!(start_transfer_sp.name, "B");
    assert_eq!(transfer_sec.from_datetime, dt_from_str("20200101T090500")?);

    let end_transfer_sp = make_stop_point(&transfer_sec.to_stop_point, &model);
    assert_eq!(end_transfer_sp.name, "F");
    assert_eq!(transfer_sec.to_datetime, dt_from_str("20200101T090700")?);

    // Waiting section
    let waiting_sec = &journey.connections[0].1;
    let sp_waiting_section = make_stop_point(&waiting_sec.stop_point, &model);
    assert_eq!(sp_waiting_section.name, "F");
    assert_eq!(waiting_sec.from_datetime, dt_from_str("20200101T090700")?);
    assert_eq!(waiting_sec.to_datetime, dt_from_str("20200101T092001")?);

    // vehicle section
    let vehicle_sec = &journey.connections[0].2;
    let (from_sp, to_sp) = make_pt_from_vehicle(vehicle_sec, &model)?;
    assert_eq!(from_sp.name, "F");
    assert_eq!(to_sp.name, "G");
    assert_eq!(vehicle_sec.from_datetime, dt_from_str("20200101T092001")?);
    assert_eq!(vehicle_sec.to_datetime, dt_from_str("20200101T093000")?);

    Ok(())
}

#[test]
fn test_routing_backward() -> Result<(), Error> {
    init();

    let model = ModelBuilder::new("20200101", "20200102")?
        .calendar("service1", &["2020-01-01"])
        .vj("toto", |vj_builder| {
            vj_builder
                .calendar("service1")
                .st("A", "10:00:00", "10:00:01")
                .st("B", "10:05:00", "10:05:01")
                .st("C", "10:10:00", "10:10:01");
        })
        .vj("tata", |vj_builder| {
            vj_builder
                .calendar("service1")
                .st("E", "10:05:00", "10:05:01")
                .st("F", "10:20:00", "10:20:01")
                .st("G", "10:30:00", "10:30:01");
        })
        .add_transfer("B".into(), "F".into(), 120)
        .build();

    let mut config = Config::new("20200101T104000", "A", "G");
    config.datetime_represent = DateTimeRepresent::Arrival;

    let responses = build_and_solve::<PeriodicData>(&model, &loki::LoadsData::empty(), &config)?;

    assert_eq!(model.vehicle_journeys.len(), 2);
    assert_eq!(responses.len(), 1);

    let journey = &responses[0];
    assert_eq!(journey.first_vj_uri(&model), "toto");
    assert_eq!(journey.nb_of_sections(), 4);

    assert_eq!(journey.nb_of_transfers(), 1);
    assert_eq!(journey.total_duration(), 1799);

    // First Vehicle
    let vehicle_sec = &journey.first_vehicle;
    let (from_sp, to_sp) = make_pt_from_vehicle(vehicle_sec, &model)?;
    assert_eq!(from_sp.name, "A");
    assert_eq!(to_sp.name, "B");
    assert_eq!(vehicle_sec.from_datetime, dt_from_str("20200101T090001")?);
    assert_eq!(vehicle_sec.to_datetime, dt_from_str("20200101T090500")?);

    // Transfer section
    assert_eq!(journey.connections.len(), 1);
    let transfer_sec = &journey.connections[0].0;
    let start_transfer_sp = make_stop_point(&transfer_sec.from_stop_point, &model);
    assert_eq!(start_transfer_sp.name, "B");
    assert_eq!(transfer_sec.from_datetime, dt_from_str("20200101T090500")?);

    let end_transfer_sp = make_stop_point(&transfer_sec.to_stop_point, &model);
    assert_eq!(end_transfer_sp.name, "F");
    assert_eq!(transfer_sec.to_datetime, dt_from_str("20200101T090700")?);

    // Waiting section
    let waiting_sec = &journey.connections[0].1;
    let sp_waiting_section = make_stop_point(&waiting_sec.stop_point, &model);
    assert_eq!(sp_waiting_section.name, "F");
    assert_eq!(waiting_sec.from_datetime, dt_from_str("20200101T090700")?);
    assert_eq!(waiting_sec.to_datetime, dt_from_str("20200101T092001")?);

    // vehicle section
    let vehicle_sec = &journey.connections[0].2;
    let (from_sp, to_sp) = make_pt_from_vehicle(vehicle_sec, &model)?;
    assert_eq!(from_sp.name, "F");
    assert_eq!(to_sp.name, "G");
    assert_eq!(vehicle_sec.from_datetime, dt_from_str("20200101T092001")?);
    assert_eq!(vehicle_sec.to_datetime, dt_from_str("20200101T093000")?);

    Ok(())
}
