// Copyright  (C) 2021, Hove and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
// the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
// powered by Hove (www.kisio.com).
// Help us simplify mobility and open public transport:
// a non ending quest to the responsive locomotion way of traveling!
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

pub use loki_server;
use loki_server::{chaos_proto, navitia_proto, server_config::ServerConfig};

use chaos_proto::gtfs_realtime as gtfs_proto;
use gtfs_proto::FeedHeader;
use loki_launch::loki::{
    chrono::{NaiveTime, Timelike, Utc},
    models::real_time_disruption::time_periods::TimePeriod,
};
use protobuf::{Message, MessageField};

use crate::{datetime, first_section_vj_name, reload_base_data, wait_until_realtime_updated_after};

#[derive(Debug)]
enum PtObject<'a> {
    Network(&'a str),
    Line(&'a str),
    Route(&'a str),
    Trip(&'a str),
    StopPoint(&'a str),
    StopArea(&'a str),
}

// Reload choas database and check if all required information's are correctly loaded
// and transformed into loki::Disruption
pub async fn load_database_test(config: &ServerConfig) {
    let date_time = datetime("2021-01-01 18:00:00");

    // initial request
    let journey_request =
        crate::make_journeys_request("stop_point:pontoise", "stop_point:dourdan", date_time);

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            journey_request.clone(),
        )
        .await;
        // info!("{:#?}", journeys_response);
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .pt_display_informations
                .as_ref()
                .unwrap()
                .uris
                .as_ref()
                .unwrap()
                .vehicle_journey
                .as_ref()
                .unwrap(),
            "vehicle_journey:rer_c_soir"
        );
        // We should get a disruption in journeys_response, that was loaded from the chaos database
        // We ccheck that informations contained in this disruption matche with thoses in database
        assert_eq!(journeys_response.impacts.len(), 1);
        let impact = &journeys_response.impacts[0];
        assert_eq!(
            impact.uri.as_ref().unwrap(),
            "ffffffff-ffff-ffff-ffff-ffffffffffff"
        );
        assert_eq!(
            impact.disruption_uri.as_ref().unwrap(),
            "dddddddd-dddd-dddd-dddd-dddddddddddd"
        );
        assert_eq!(impact.contributor.as_ref().unwrap(), "test_realtime_topic");
        let updated_at = datetime("2018-08-28 15:50:08");
        assert_eq!(impact.updated_at.unwrap(), updated_at.timestamp() as u64);

        assert_eq!(impact.application_periods.len(), 1);
        let application_periods = &impact.application_periods[0];
        let begin = datetime("2021-01-01 14:00:00");
        let end = datetime("2021-01-02 22:00:00");
        assert_eq!(application_periods.begin.unwrap(), begin.timestamp() as u64);
        assert_eq!(application_periods.end.unwrap(), end.timestamp() as u64);

        assert_eq!(impact.cause.as_ref().unwrap(), "cause_wording");
        assert_eq!(impact.category.as_ref().unwrap(), "Cat name");
        assert_eq!(impact.tags, vec!["prolongation".to_string()]);

        assert_eq!(impact.messages.len(), 1);
        let message = &impact.messages[0];
        assert_eq!(message.text.as_ref().unwrap(), "Test Message");
        let channel = message.channel.as_ref().unwrap();
        assert_eq!(
            channel.id.as_ref().unwrap(),
            "fd4cec38-669d-11e5-b2c1-005056a40962"
        );
        assert_eq!(channel.name.as_ref().unwrap(), "web et mobile");
        assert_eq!(channel.content_type.as_ref().unwrap(), "text/html");

        let severity = impact.severity.as_ref().unwrap();
        assert_eq!(severity.name.as_ref().unwrap(), "accident");
        assert_eq!(severity.color.as_ref().unwrap(), "#99DD66");
        assert_eq!(
            severity.effect.unwrap(),
            navitia_proto::severity::Effect::NoService as i32
        );
        assert_eq!(severity.priority.unwrap(), 4);
        assert_eq!(impact.properties.len(), 1);
        let property = &impact.properties[0];
        assert_eq!(&property.key, "ccb9e71f-619c-4972-97cd-ae506d31852d");
        assert_eq!(&property.r#type, "Property Test");
        assert_eq!(&property.value, "property value test");

        assert_eq!(impact.application_patterns.len(), 1);
        let pattern = &impact.application_patterns[0];
        let begin = datetime("2021-01-01 00:00:00");
        let end = datetime("2021-01-02 00:00:00");
        assert_eq!(
            pattern.application_period.begin.unwrap(),
            begin.timestamp() as u64
        );
        assert_eq!(
            pattern.application_period.end.unwrap(),
            end.timestamp() as u64
        );

        assert_eq!(pattern.time_slots.len(), 1);
        let time_slot = &pattern.time_slots[0];
        let begin = NaiveTime::from_hms_opt(14, 00, 00).unwrap();
        let end = NaiveTime::from_hms_opt(22, 00, 00).unwrap();
        assert_eq!(time_slot.begin, begin.num_seconds_from_midnight());
        assert_eq!(time_slot.end, end.num_seconds_from_midnight());

        let week_pattern = &pattern.week_pattern;
        assert_eq!(week_pattern.monday, Some(true));
        assert_eq!(week_pattern.tuesday, Some(true));
        assert_eq!(week_pattern.wednesday, Some(false));
        assert_eq!(week_pattern.thursday, Some(true));
        assert_eq!(week_pattern.friday, Some(true));
        assert_eq!(week_pattern.saturday, Some(false));
        assert_eq!(week_pattern.sunday, Some(false));
    }

    // let's make the same request, but on the realtime level
    // we should get no journey in the response
    // because the chaos disruption stored in the chaos database
    // has a NO_SERVICE effect
    {
        let mut realtime_request = journey_request.clone();
        realtime_request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
}

// try to remove all vehicle of a network
// but on a period that don't intersect with calendar validity_period
pub async fn delete_network_on_invalid_period_test(config: &ServerConfig) {
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's delete all Trip of "my_network" Network
    // between 2021-02-01 and 2021-02-01
    let dt_period = TimePeriod::new(
        datetime("2021-02-01 00:00:00"),
        datetime("2021-02-01 23:59:59"),
    )
    .unwrap();
    let send_realtime_message_datetime = Utc::now().naive_utc();
    let realtime_message = create_no_service_disruption(
        &PtObject::Network("my_network"),
        &dt_period,
        "no_service_on_my_network",
    );
    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // wait until realtime message is taken into account
    crate::wait_until_realtime_updated_after(
        &config.requests_socket,
        &send_realtime_message_datetime,
    )
    .await;

    // let's make the same request, but on the realtime level
    // we should get a journey in the response
    // because the disruption previously sent had no effect
    // due to application period
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
    }
}

pub async fn delete_vj_test(config: &ServerConfig) {
    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's delete the only trip
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::Trip("matin"),
        &dt_period,
        "no_service_on_trip_matin",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get no journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }
}

pub async fn delete_line_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");
    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's delete the only trip
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::Line("rer_b"),
        &dt_period,
        "no_service_on_line_rer_b",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get no journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }
}

pub async fn delete_route_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's delete the only route
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::Route("rer_b_nord"),
        &dt_period,
        "no_service_on_route_rer_b_nord",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get no journey in the response an no linked impact
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }
}

pub async fn cancel_disruption_on_route_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's delete the only route
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();
    let disruption_id = "no_service_on_route_rer_b_nord";
    let realtime_message =
        create_no_service_disruption(&PtObject::Route("rer_b_nord"), &dt_period, disruption_id);
    crate::send_realtime_message_and_wait_until_reception(config, realtime_message.clone()).await;

    // let's make the  request, but the realtime level
    // we should get no journey since the vj should be deleted
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // then revert previously sent disruption
    let cancel_realtime_message = create_cancel_disruption(disruption_id);
    crate::send_realtime_message_and_wait_until_reception(config, cancel_realtime_message).await;

    // let's make a request on the realtime level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }
}

pub async fn delete_stop_point_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's mark the StopPoint 'massy' as not in service
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::StopPoint("stop_point:massy"),
        &dt_period,
        "no_service_on_stop_point_massy",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get no journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
        assert_eq!(journeys_response.impacts.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response with a linked impact
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
        assert_eq!(
            journeys_response.impacts[0].impacted_objects[0]
                .pt_object
                .as_ref()
                .unwrap()
                .uri,
            "stop_point:massy"
        );
    }
}

pub async fn delete_stop_point_on_invalid_period_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // the vehicle circulate at 8:00 at massy
    // so if the application_period of the disruption
    // starts at 8:30, it should not remove the vehicle
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 08:30:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::StopPoint("stop_point:massy"),
        &dt_period,
        "no_service_on_stop_point_massy",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
    }
}

pub async fn delete_several_stop_point_and_then_cancel_disruption_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains
    // a vehicle_journey named "matin" with stop_times :
    //  - "massy" at 8h
    //  - "paris" at 9h
    //  - "cdg" at  9h30
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:cdg", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's mark the StopPoint 'paris' as not in service
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();
    let delete_massy_disruption_id = "no_service_on_stop_point_massy";
    let realtime_message = create_no_service_disruption(
        &PtObject::StopPoint("stop_point:paris"),
        &dt_period,
        delete_massy_disruption_id,
    );
    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the realtime level request
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
        // since we removed the stop_point paris, we should get just 2 stop_times (massy and cdg)
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .stop_date_times
                .len(),
            2
        );
        assert_eq!(journeys_response.impacts.len(), 1);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response with a linked impact
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
        // on the base level, we should get 3 stop_times
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .stop_date_times
                .len(),
            3
        );
        assert_eq!(
            journeys_response.impacts[0].impacted_objects[0]
                .pt_object
                .as_ref()
                .unwrap()
                .uri,
            "stop_point:paris"
        );
    }

    // let's mark now the StopPoint 'cdg' as not in service
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();
    let delete_cdg_disruption_id = "no_service_on_stop_point_cdg";

    let realtime_message = create_no_service_disruption(
        &PtObject::StopPoint("stop_point:cdg"),
        &dt_period,
        delete_cdg_disruption_id,
    );
    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the realtime level request
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        // since we removed the stop_point cdg, we should get no journey
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response with 2 linked impacts
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
        // on the base level, we should get 3 stop_times
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .stop_date_times
                .len(),
            3
        );
        assert_eq!(journeys_response.impacts.len(), 2);
    }

    // let's cancel the disruption on stop_point cdg
    let cancel_realtime_message = create_cancel_disruption(delete_cdg_disruption_id);
    crate::send_realtime_message_and_wait_until_reception(config, cancel_realtime_message).await;

    // let's make the realtime level request
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
        // since we removed the stop_point paris, we should get just 2 stop_times (massy and cdg)
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .stop_date_times
                .len(),
            2
        );
        assert_eq!(journeys_response.impacts.len(), 1);
    }

    // let's cancel the disruption on stop_point massy
    let cancel_realtime_message = create_cancel_disruption(delete_massy_disruption_id);
    crate::send_realtime_message_and_wait_until_reception(config, cancel_realtime_message).await;

    // let's make the realtime level request
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 1);
        //  we should get just 3 stop_times (massy, paris and cdg)
        assert_eq!(
            journeys_response.journeys[0].sections[0]
                .stop_date_times
                .len(),
            3
        );
        // we should get no impact, since both have been cancelled
        assert_eq!(journeys_response.impacts.len(), 0);
    }
}

pub async fn delete_stop_area_test(config: &ServerConfig) {
    // let's reload the data to forget about previous disruptions
    // We must wait for chaos to be loaded in order to not send realtime message
    // before chaos database loading
    let reload_data_datetime = Utc::now().naive_utc();
    reload_base_data(config).await;
    wait_until_realtime_updated_after(&config.requests_socket, &reload_data_datetime).await;

    // the ntfs (in tests/a_small_ntfs) contains just one trip
    // with a vehicle_journey named "matin"
    // departing from "massy" at 8h and arriving to "paris" at 9h
    // on day 2021-01-01
    let request_datetime = datetime("2021-01-01 08:00:00");

    // initial request, on base schedule
    let base_request =
        crate::make_journeys_request("stop_point:massy", "stop_point:paris", request_datetime);

    // same request, but on the realtime level
    let realtime_request = {
        let mut request = base_request.clone();
        request
            .journeys
            .as_mut()
            .unwrap()
            .set_realtime_level(navitia_proto::RtLevel::Realtime);
        request
    };

    // let's first check that we do get a response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        // check that we have a journey, that uses the only trip in the ntfs
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
    }

    // let's delete the only trip
    let dt_period = TimePeriod::new(
        datetime("2021-01-01 00:00:00"),
        datetime("2021-01-01 23:00:00"),
    )
    .unwrap();

    let realtime_message = create_no_service_disruption(
        &PtObject::StopArea("stop_area:massy_area"),
        &dt_period,
        "no_service_on_stop_area_massy",
    );

    crate::send_realtime_message_and_wait_until_reception(config, realtime_message).await;

    // let's make the same request, but on the realtime level
    // we should get no journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            realtime_request.clone(),
        )
        .await;
        assert_eq!(journeys_response.journeys.len(), 0);
    }
    // with the same request on the 'base schedule' level
    // we should get a journey in the response
    {
        let journeys_response = crate::send_request_and_wait_for_response(
            &config.requests_socket,
            base_request.clone(),
        )
        .await;
        assert_eq!(
            first_section_vj_name(&journeys_response.journeys[0]),
            "vehicle_journey:matin"
        );
        assert_eq!(
            journeys_response.impacts[0].uri.as_ref().unwrap(),
            "no_service_on_stop_area_massy"
        );
        assert_eq!(
            journeys_response.impacts[0].impacted_objects[0]
                .pt_object
                .as_ref()
                .unwrap()
                .uri,
            "massy_area"
        );
    }
}

fn create_no_service_disruption(
    pt_object: &PtObject,
    application_period: &TimePeriod,
    disruption_id: &str,
) -> gtfs_proto::FeedMessage {
    let id = disruption_id.to_string();

    let mut entity = chaos_proto::chaos::PtObject::new();
    match pt_object {
        PtObject::Network(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::network);
            entity.set_uri(id.to_string());
        }
        PtObject::Route(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::route);
            entity.set_uri(id.to_string());
        }
        PtObject::Line(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::line);
            entity.set_uri(id.to_string());
        }
        PtObject::Trip(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::trip);
            entity.set_uri(id.to_string());
        }
        PtObject::StopArea(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::stop_area);
            entity.set_uri(id.to_string());
        }
        PtObject::StopPoint(id) => {
            entity.set_pt_object_type(chaos_proto::chaos::pt_object::Type::stop_point);
            entity.set_uri(id.to_string());
        }
    }

    let mut period = gtfs_proto::TimeRange::default();
    period.set_start(application_period.start().timestamp() as u64);
    period.set_end(application_period.end().timestamp() as u64);

    let mut channel = chaos_proto::chaos::Channel::default();
    channel.set_id("disruption test sample".to_string());
    channel.set_name("web".to_string());
    channel.set_content_type("html".to_string());
    channel.set_max_size(250);
    channel
        .types
        .push(chaos_proto::chaos::channel::Type::web.into());

    let mut message = chaos_proto::chaos::Message::default();
    message.set_text("disruption test sample".to_string());
    message.channel = MessageField::<chaos_proto::chaos::Channel>::some(channel);

    let mut severity = chaos_proto::chaos::Severity::default();
    severity.set_id("severity id for NO_SERVICE".to_string());
    severity.set_wording("severity wording for NO_SERVICE".to_string());
    severity.set_color("#FF0000".to_string());
    severity.set_priority(10);
    severity.set_effect(gtfs_proto::alert::Effect::NO_SERVICE);

    let mut impact = chaos_proto::chaos::Impact::default();
    impact.set_id(id.clone());
    impact.set_created_at(Utc::now().timestamp() as u64);
    impact.set_updated_at(Utc::now().timestamp() as u64);
    impact.informed_entities.push(entity);
    impact.application_periods.push(period.clone());
    impact.messages.push(message);
    impact.severity = MessageField::<chaos_proto::chaos::Severity>::some(severity);

    let mut cause = chaos_proto::chaos::Cause::default();
    cause.set_id("disruption cause test".to_string());
    cause.set_wording("disruption cause test".to_string());
    let mut category = chaos_proto::chaos::Category::default();
    category.set_id("disruption cause category test".to_string());
    category.set_name("disruption cause category test".to_string());
    cause.category = MessageField::<chaos_proto::chaos::Category>::some(category);

    let mut disruption = chaos_proto::chaos::Disruption::default();
    disruption.set_id(id.clone());
    disruption.set_reference("ChaosDisruptionTest".to_string());
    disruption.publication_period = MessageField::<gtfs_proto::TimeRange>::some(period.clone());
    disruption.cause = MessageField::<chaos_proto::chaos::Cause>::some(cause);
    disruption.impacts.push(impact);

    // put the update in a feed_entity
    let mut feed_entity = gtfs_proto::FeedEntity::new();
    feed_entity.set_id(id);
    let vec: Vec<u8> = disruption.write_to_bytes().expect("cannot write message");
    feed_entity
        .mut_unknown_fields()
        // 1000 is the field number of `disruption` in `FeedEntity`
        // We used to be able to no hardcode the value in `protobuf:2`
        // https://github.com/stepancheg/rust-protobuf/discussions/623
        .add_length_delimited(1000, vec);

    let mut feed_header = gtfs_proto::FeedHeader::new();
    feed_header.set_gtfs_realtime_version("1.0".to_string());
    let timestamp = datetime("2022-01-01 12:00:00").timestamp();
    feed_header.set_timestamp(u64::try_from(timestamp).unwrap());

    let mut feed_message = gtfs_proto::FeedMessage::new();
    feed_message.entity.push(feed_entity);
    feed_message.header = MessageField::<FeedHeader>::some(feed_header);

    feed_message
}

fn create_cancel_disruption(id_of_disruption_to_cancel: &str) -> gtfs_proto::FeedMessage {
    // put the update in a feed_entity
    let mut feed_entity = gtfs_proto::FeedEntity::new();
    feed_entity.set_id(id_of_disruption_to_cancel.to_string());
    feed_entity.set_is_deleted(true);

    let mut feed_header = gtfs_proto::FeedHeader::new();
    feed_header.set_gtfs_realtime_version("1.0".to_string());
    let timestamp = datetime("2022-01-01 12:00:00").timestamp();
    feed_header.set_timestamp(u64::try_from(timestamp).unwrap());

    let mut feed_message = gtfs_proto::FeedMessage::new();
    feed_message.entity.push(feed_entity);
    feed_message.header = MessageField::<FeedHeader>::some(feed_header);

    feed_message
}
