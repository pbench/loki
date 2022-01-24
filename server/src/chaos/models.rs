use crate::{
    chaos::sql_types::{
        ChannelType as ChannelTypeSQL, DisruptionStatus, ImpactStatus, PtObjectType, SeverityEffect,
    },
    chaos_proto, info,
    server_config::ChaosParams,
};
use anyhow::{bail, Context, Error};
use diesel::{
    pg::types::sql_types::Array,
    prelude::*,
    sql_types::{Bit, Bool, Date, Int4, Nullable, Text, Time, Timestamp, Uuid},
};
use launch::loki::{
    chrono::{NaiveDate, NaiveTime, Timelike},
    models::real_time_disruption::BlockedStopArea,
    tracing::error,
    NaiveDateTime,
};
use std::collections::{hash_map::Entry::Vacant, HashMap, HashSet};
use uuid::Uuid as Uid;

pub fn read_chaos_disruption_from_database(
    config: &ChaosParams,
    publication_period: (NaiveDate, NaiveDate),
    contributors: &[String],
) -> Result<Vec<chaos_proto::chaos::Disruption>, Error> {
    let connection = PgConnection::establish(&config.chaos_database)?;

    let mut disruption_maker = DisruptionMaker::default();

    let mut offset_query = 0_u16;

    loop {
        info!("Querying chaos database");
        let res = diesel::sql_query(include_str!("query.sql"))
            .bind::<Date, _>(publication_period.1)
            .bind::<Date, _>(publication_period.0)
            .bind::<Date, _>(publication_period.1)
            .bind::<Array<Text>, _>(contributors)
            .bind::<Int4, _>(i32::from(config.chaos_batch_size))
            .bind::<Int4, _>(i32::from(offset_query))
            .load::<ChaosRow>(&connection);
        // Increment offset in query
        offset_query += config.chaos_batch_size;

        let rows = res?;
        if rows.is_empty() {
            break;
        }

        info!("Converting database rows into Disruption");
        for row in rows {
            if let Err(ref err) = disruption_maker.read_disruption(&row) {
                error!("{}", err);
            }
        }
    }

    info!("Disruptions ready to be applied");

    Ok(disruption_maker.disruptions.into_values().collect())
}

// Each ChaosRow contains only a part of a Disruption
// So In order to construct a Disruption we parse & merge each ChaosRow
// this is the main goal of DisruptionMaker
#[derive(Default)]
struct DisruptionMaker {
    // When we receive a ChaosRow we can create a new Disruption if the disruption id is new (ie not in disruptions_set)
    // or we update an already created Disruption stored in disruptions HashMap
    pub(crate) disruptions: HashMap<Uid, chaos_proto::chaos::Disruption>,

    // For each disruption we can have multiple impact, tag and property
    // In order to push unique impact, tag and property we use HashSet/HashMap
    // to check if the corresponding tag, impact, property has already been pushed in a disruption
    // Note : tags and property do not need to be completed/updated
    // but an impact can be completed so we use a HashMap<Uid, usize>
    // in order to locate the correct position of impact in disruption.impacts[] vector
    tags_set: HashSet<Uid>,
    properties_set: HashSet<(String, String, String)>, //(type, key, value)
    impacts_set: HashMap<Uid, usize>,

    impact_object_set: ImpactMaker,
}

impl DisruptionMaker {
    pub fn read_disruption(&mut self, row: &ChaosRow) -> Result<(), Error> {
        let find_disruption = self.disruptions.entry(row.disruption_id);
        if let Vacant(entry) = find_disruption {
            let disruption = DisruptionMaker::make_disruption(row)?;

            // clear all set related to disruption
            self.impacts_set.clear();
            self.tags_set.clear();
            self.properties_set.clear();
            self.impact_object_set.clear();

            entry.insert(disruption);
        }
        // after previous insert unwrap is safe here!
        let disruption = self.disruptions.get_mut(&row.disruption_id).unwrap();

        DisruptionMaker::update_tags(&mut self.tags_set, row, disruption)?;
        DisruptionMaker::update_properties(&mut self.properties_set, row, disruption)?;
        DisruptionMaker::update_impacts(
            &mut self.impact_object_set,
            &mut self.impacts_set,
            row,
            disruption,
        )?;
        Ok(())
    }

    fn make_disruption(row: &ChaosRow) -> Result<chaos_proto::chaos::Disruption, Error> {
        let mut disruption = chaos_proto::chaos::Disruption::new();
        disruption.set_id(row.disruption_id.to_string());
        disruption.set_contributor(row.contributor.clone());
        if let Some(reference) = &row.disruption_reference {
            disruption.set_reference(reference.clone());
        }
        disruption.set_created_at(u64::try_from(row.disruption_created_at.timestamp())?);
        if let Some(updated_at) = &row.disruption_updated_at {
            disruption.set_updated_at(u64::try_from(updated_at.timestamp())?);
        }
        // Fill cause
        let cause = disruption.mut_cause();
        cause.set_wording(row.cause_wording.clone());
        if let Some(category_name) = &row.category_name {
            let category = cause.mut_category();
            category.set_name(category_name.clone());
        }
        // Fill publication_period
        let publication_period = disruption.mut_publication_period();
        if let Some(start) = &row.disruption_start_publication_date {
            publication_period.set_start(u64::try_from(start.timestamp())?);
        }
        if let Some(end) = &row.disruption_end_publication_date {
            publication_period.set_end(u64::try_from(end.timestamp())?);
        }
        Ok(disruption)
    }

    fn update_tags(
        tags_set: &mut HashSet<Uid>,
        row: &ChaosRow,
        disruption: &mut chaos_proto::chaos::Disruption,
    ) -> Result<(), Error> {
        if let Some(tag_id) = row.tag_id {
            if tags_set.insert(tag_id) {
                let mut tag = chaos_proto::chaos::Tag::new();
                tag.set_id(tag_id.to_string());
                if let Some(name) = &row.tag_name {
                    tag.set_name(name.clone());
                }
                disruption.tags.push(tag);
            }
        }
        Ok(())
    }

    fn update_properties(
        properties_set: &mut HashSet<(String, String, String)>,
        row: &ChaosRow,
        disruption: &mut chaos_proto::chaos::Disruption,
    ) -> Result<(), Error> {
        // type_ is here like an Uuid
        if let Some(type_) = &row.property_type {
            let key = if let Some(key) = &row.property_key {
                key
            } else {
                bail!("Property has no key");
            };
            let value = if let Some(value) = &row.property_value {
                value
            } else {
                bail!("Property has no value");
            };
            let tuple = (type_.clone(), key.clone(), value.clone());
            if properties_set.insert(tuple) {
                let mut property = chaos_proto::chaos::DisruptionProperty::new();
                property.set_field_type(type_.clone());
                property.set_key(key.clone());
                property.set_value(value.clone());
                disruption.properties.push(property);
            }
        }
        Ok(())
    }

    fn update_impacts(
        impact_object_set: &mut ImpactMaker,
        impacts_set: &mut HashMap<Uid, usize>,
        row: &ChaosRow,
        disruption: &mut chaos_proto::chaos::Disruption,
    ) -> Result<(), Error> {
        let impact = if let Some(idx) = impacts_set.get(&row.impact_id) {
            // Impact already in disruption
            disruption.impacts.get_mut(*idx).unwrap()
        } else {
            // Or create a new impact We must then clear all  sub-objects sets belonging to impact
            let impact = ImpactMaker::make_impact(row)?;
            impact_object_set.clear();

            disruption.impacts.push(impact);
            let idx: usize = disruption.impacts.len() - 1;
            impacts_set.insert(row.impact_id, idx);
            disruption.impacts.last_mut().unwrap()
        };

        ImpactMaker::update_application_period(
            &mut impact_object_set.application_periods_set,
            row,
            impact,
        )?;
        ImpactMaker::update_messages(&mut impact_object_set.messages_set, row, impact)?;
        ImpactMaker::update_application_pattern(
            &mut impact_object_set.application_pattern_set,
            row,
            impact,
        )?;
        ImpactMaker::update_pt_objects(&mut impact_object_set.pt_object_set, row, impact)?;

        Ok(())
    }

    fn make_severity_effect(effect: &SeverityEffect) -> chaos_proto::gtfs_realtime::Alert_Effect {
        use chaos_proto::gtfs_realtime::Alert_Effect;
        match effect {
            SeverityEffect::NoService => Alert_Effect::NO_SERVICE,
            SeverityEffect::OtherEffect => Alert_Effect::OTHER_EFFECT,
            SeverityEffect::ModifiedService => Alert_Effect::MODIFIED_SERVICE,
            SeverityEffect::AdditionalService => Alert_Effect::ADDITIONAL_SERVICE,
            SeverityEffect::StopMoved => Alert_Effect::STOP_MOVED,
            SeverityEffect::SignificantDelays => Alert_Effect::SIGNIFICANT_DELAYS,
            SeverityEffect::ReducedService => Alert_Effect::REDUCED_SERVICE,
            SeverityEffect::UnknownEffect => Alert_Effect::UNKNOWN_EFFECT,
            SeverityEffect::Detour => Alert_Effect::DETOUR,
        }
    }
}

#[derive(Default)]
struct ImpactMaker {
    application_periods_set: HashSet<Uid>,
    application_pattern_set: HashSet<Uid>,
    messages_set: HashSet<Uid>,
    pt_object_set: HashSet<String>,
}

impl ImpactMaker {
    fn clear(&mut self) {
        self.application_periods_set.clear();
        self.application_pattern_set.clear();
        self.messages_set.clear();
    }

    fn make_impact(row: &ChaosRow) -> Result<chaos_proto::chaos::Impact, Error> {
        let mut impact = chaos_proto::chaos::Impact::new();
        impact.set_id(row.impact_id.to_string());
        impact.set_created_at(u64::try_from(row.impact_created_at.timestamp())?);
        if let Some(updated_at) = &row.impact_updated_at {
            impact.set_updated_at(u64::try_from(updated_at.timestamp())?);
        }
        // Fill severity
        let severity = impact.mut_severity();
        severity.set_id(row.severity_id.to_string());
        severity.set_wording(row.severity_wording.clone());
        severity.set_priority(row.severity_priority);
        if let Some(color) = &row.severity_color {
            severity.set_color(color.clone())
        }
        let effect = row
            .severity_effect
            .as_ref()
            .unwrap_or(&SeverityEffect::UnknownEffect);
        severity.set_effect(DisruptionMaker::make_severity_effect(effect));
        Ok(impact)
    }

    fn update_application_period(
        application_periods_set: &mut HashSet<Uid>,
        row: &ChaosRow,
        impact: &mut chaos_proto::chaos::Impact,
    ) -> Result<(), Error> {
        if application_periods_set.insert(row.application_id) {
            let mut application_period = chaos_proto::gtfs_realtime::TimeRange::new();
            if let Some(start) = &row.disruption_start_publication_date {
                application_period.set_start(u64::try_from(start.timestamp())?);
            }
            if let Some(end) = &row.disruption_end_publication_date {
                application_period.set_end(u64::try_from(end.timestamp())?);
            }
            impact.application_periods.push(application_period);
        }
        Ok(())
    }

    fn update_messages(
        messages_set: &mut HashSet<Uid>,
        row: &ChaosRow,
        impact: &mut chaos_proto::chaos::Impact,
    ) -> Result<(), Error> {
        if let Some(message_id) = row.message_id {
            if messages_set.insert(message_id) {
                let mut message = chaos_proto::chaos::Message::new();
                if let Some(text) = &row.message_text {
                    message.set_text(text.clone());
                }
                let channel = message.mut_channel();
                if let Some(name) = &row.channel_name {
                    channel.set_name(name.clone())
                }
                if let Some(content_type) = &row.channel_content_type {
                    channel.set_content_type(content_type.clone())
                }
                for channel_type in row.channel_type.iter().flatten() {
                    channel
                        .types
                        .push(ImpactMaker::make_channel_type(channel_type))
                }
                impact.messages.push(message);
            }
        }
        Ok(())
    }

    fn make_channel_type(channel_type: &ChannelTypeSQL) -> chaos_proto::chaos::Channel_Type {
        use chaos_proto::chaos::Channel_Type;
        match channel_type {
            ChannelTypeSQL::Title => Channel_Type::title,
            ChannelTypeSQL::Beacon => Channel_Type::beacon,
            ChannelTypeSQL::Twitter => Channel_Type::twitter,
            ChannelTypeSQL::Notification => Channel_Type::notification,
            ChannelTypeSQL::Sms => Channel_Type::sms,
            ChannelTypeSQL::Facebook => Channel_Type::facebook,
            ChannelTypeSQL::Email => Channel_Type::email,
            ChannelTypeSQL::Mobile => Channel_Type::mobile,
            ChannelTypeSQL::Web => Channel_Type::web,
        }
    }

    fn update_application_pattern(
        application_pattern_set: &mut HashSet<Uid>,
        row: &ChaosRow,
        impact: &mut chaos_proto::chaos::Impact,
    ) -> Result<(), Error> {
        if let Some(pattern_id) = row.pattern_id {
            if application_pattern_set.insert(pattern_id) {
                let mut pattern = chaos_proto::chaos::Pattern::new();
                if let Some(start_date) = row.pattern_start_date {
                    pattern.set_start_date(u32::try_from(start_date.and_hms(0, 0, 0).timestamp())?)
                }
                if let Some(end_date) = row.pattern_end_date {
                    pattern.set_end_date(u32::try_from(end_date.and_hms(0, 0, 0).timestamp())?)
                }
                // time_slot_begin && time_slot_end have always the same size
                // even after filter_map
                // thanks to the sql query
                let time_slots_iter = row
                    .time_slot_begin
                    .iter()
                    .flatten()
                    .zip(row.time_slot_end.iter().flatten());

                for (begin, end) in time_slots_iter {
                    let mut time_slot = chaos_proto::chaos::TimeSlot::new();
                    time_slot.set_begin(begin.num_seconds_from_midnight());
                    time_slot.set_end(end.num_seconds_from_midnight());
                    pattern.time_slots.push(time_slot);
                }
                impact.application_patterns.push(pattern);
            }
        }
        Ok(())
    }

    fn update_pt_objects(
        pt_object_set: &mut HashSet<String>,
        row: &ChaosRow,
        impact: &mut chaos_proto::chaos::Impact,
    ) -> Result<(), Error> {
        let id = if let Some(id) = &row.ptobject_uri {
            id.clone()
        } else {
            bail!("PtObject has no uri");
        };

        let pt_object_type = row.ptobject_type.clone();

        // Early exit if we already pushed a pt_object in impacts.informed_entities[]
        // except for Line/rail section (they can be updated)
        if pt_object_set.contains(&id)
            && pt_object_type != PtObjectType::RailSection
            && pt_object_type != PtObjectType::LineSection
        {
            return Ok(());
        }
        pt_object_set.insert(id.clone());

        use chaos_proto::chaos::PtObject_Type;
        match pt_object_type {
            PtObjectType::LineSection => {
                // check if we need to create a new line section
                // or just update it ie. push a new route into it
                let found_line_section = impact
                    .informed_entities
                    .iter_mut()
                    .filter(|pt_object| {
                        pt_object.get_pt_object_type() == PtObject_Type::line_section
                    })
                    .find(|pt_object| pt_object.get_uri() == id);

                match found_line_section {
                    Some(pt_object) => {
                        // we found line_section so we push a new route
                        // if not already in line_section.routes[]
                        if let Some(route_id) = &row.ls_route_uri {
                            let found_route = pt_object
                                .get_pt_line_section()
                                .routes
                                .iter()
                                .find(|route| route.get_uri() == *route_id);
                            if found_route.is_none() {
                                let mut route = chaos_proto::chaos::PtObject::new();
                                route.set_uri(route_id.clone());
                                route.set_pt_object_type(PtObject_Type::route);
                                pt_object.mut_pt_line_section().routes.push(route);
                            }
                        }
                    }
                    None => {
                        let mut line_section = chaos_proto::chaos::LineSection::new();
                        if let Some(line_id) = &row.ls_line_uri {
                            let mut line = chaos_proto::chaos::PtObject::new();
                            line.set_uri(line_id.clone());
                            line.set_pt_object_type(PtObject_Type::line);
                            line_section.set_line(line);
                        }
                        if let Some(start) = &row.ls_start_uri {
                            let mut start_stop = chaos_proto::chaos::PtObject::new();
                            start_stop.set_uri(start.clone());
                            start_stop.set_pt_object_type(PtObject_Type::stop_area);
                            line_section.set_start_point(start_stop);
                        }
                        if let Some(end) = &row.ls_end_uri {
                            let mut end_stop = chaos_proto::chaos::PtObject::new();
                            end_stop.set_uri(end.clone());
                            end_stop.set_pt_object_type(PtObject_Type::stop_area);
                            line_section.set_end_point(end_stop);
                        }
                        if let Some(route_id) = &row.ls_route_uri {
                            let mut route = chaos_proto::chaos::PtObject::new();
                            route.set_uri(route_id.clone());
                            route.set_pt_object_type(PtObject_Type::route);
                            line_section.routes.push(route);
                        }

                        let mut pt_object = chaos_proto::chaos::PtObject::new();
                        pt_object.set_uri(id.clone());
                        pt_object.set_pt_object_type(PtObject_Type::line_section);
                        pt_object.set_pt_line_section(line_section);
                        impact.informed_entities.push(pt_object);
                    }
                }
            }
            PtObjectType::RailSection => {
                // check if we need to create a new rail section or just push a new route into it
                let found_rail_section = impact
                    .informed_entities
                    .iter_mut()
                    .filter(|pt_object| {
                        pt_object.get_pt_object_type() == PtObject_Type::rail_section
                    })
                    .find(|pt_object| pt_object.get_uri() == id);

                match found_rail_section {
                    Some(pt_object) => {
                        // we found rail_section so we push a new route
                        // if not already in rail_section.routes[]
                        if let Some(route_id) = &row.rs_route_uri {
                            let found_route = pt_object
                                .get_pt_rail_section()
                                .routes
                                .iter()
                                .find(|route| route.get_uri() == *route_id);
                            if found_route.is_none() {
                                let mut route = chaos_proto::chaos::PtObject::new();
                                route.set_uri(route_id.clone());
                                route.set_pt_object_type(PtObject_Type::route);
                                pt_object.mut_pt_rail_section().routes.push(route);
                            }
                        }
                    }
                    None => {
                        let mut rail_section = chaos_proto::chaos::RailSection::new();
                        if let Some(line_id) = &row.rs_line_uri {
                            let mut line = chaos_proto::chaos::PtObject::new();
                            line.set_uri(line_id.clone());
                            line.set_pt_object_type(PtObject_Type::line);
                            rail_section.set_line(line);
                        }
                        if let Some(start) = &row.rs_start_uri {
                            let mut start_stop = chaos_proto::chaos::PtObject::new();
                            start_stop.set_uri(start.clone());
                            start_stop.set_pt_object_type(PtObject_Type::stop_area);
                            rail_section.set_start_point(start_stop);
                        }
                        if let Some(end) = &row.rs_end_uri {
                            let mut end_stop = chaos_proto::chaos::PtObject::new();
                            end_stop.set_uri(end.clone());
                            end_stop.set_pt_object_type(PtObject_Type::stop_area);
                            rail_section.set_end_point(end_stop);
                        }
                        if let Some(route_id) = &row.rs_route_uri {
                            let mut route = chaos_proto::chaos::PtObject::new();
                            route.set_uri(route_id.clone());
                            route.set_pt_object_type(PtObject_Type::route);
                            rail_section.routes.push(route);
                        }

                        if let Some(blocked_stop_area) = &row.rs_blocked_sa {
                            let blocked_stop_area = serde_json::from_str::<Vec<BlockedStopArea>>(
                                blocked_stop_area.as_str(),
                            )
                            .with_context(|| {
                                "Could not deserialize blocked_stop_area of rail_section"
                            })?;
                            for stop_area in blocked_stop_area {
                                let mut pt_object_ordered =
                                    chaos_proto::chaos::OrderedPtObject::new();
                                pt_object_ordered.set_uri(stop_area.id);
                                pt_object_ordered.set_order(stop_area.order);
                                rail_section.blocked_stop_areas.push(pt_object_ordered);
                            }
                        }

                        let mut pt_object = chaos_proto::chaos::PtObject::new();
                        pt_object.set_uri(id.clone());
                        pt_object.set_pt_object_type(PtObject_Type::rail_section);
                        pt_object.set_pt_rail_section(rail_section);
                        impact.informed_entities.push(pt_object);
                    }
                }
            }
            _ => {
                let mut pt_object = chaos_proto::chaos::PtObject::new();
                pt_object.set_uri(id);
                pt_object.set_pt_object_type(ImpactMaker::make_pt_object_type(&pt_object_type));
                impact.informed_entities.push(pt_object);
            }
        };

        Ok(())
    }

    fn make_pt_object_type(pt_object_type: &PtObjectType) -> chaos_proto::chaos::PtObject_Type {
        use chaos_proto::chaos::PtObject_Type;
        match pt_object_type {
            PtObjectType::StopArea => PtObject_Type::stop_area,
            PtObjectType::StopPoint => PtObject_Type::stop_point,
            PtObjectType::LineSection => PtObject_Type::line_section,
            PtObjectType::RailSection => PtObject_Type::rail_section,
            PtObjectType::Route => PtObject_Type::route,
            PtObjectType::Line => PtObject_Type::line,
            PtObjectType::Network => PtObject_Type::network,
        }
    }
}

// Remove ChaosRow when PR https://github.com/diesel-rs/diesel/pull/2254 is merged
// and use model_v2
#[derive(Queryable, QueryableByName, Debug)]
pub struct ChaosRow {
    // Disruptions field
    #[sql_type = "Uuid"]
    pub disruption_id: Uid,
    #[sql_type = "Nullable<Text>"]
    pub disruption_reference: Option<String>,
    #[sql_type = "crate::chaos::sql_types::disruption_status"]
    pub disruption_status: DisruptionStatus,
    #[sql_type = "Nullable<Timestamp>"]
    pub disruption_start_publication_date: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub disruption_end_publication_date: Option<NaiveDateTime>,
    #[sql_type = "Timestamp"]
    pub disruption_created_at: NaiveDateTime,
    #[sql_type = "Nullable<Timestamp>"]
    pub disruption_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Text"]
    pub contributor: String,
    // Cause fields
    #[sql_type = "Uuid"]
    pub cause_id: Uid,
    #[sql_type = "Text"]
    pub cause_wording: String,
    #[sql_type = "diesel::sql_types::Bool"]
    pub cause_visible: bool,
    #[sql_type = "Timestamp"]
    pub cause_created_at: NaiveDateTime,
    #[sql_type = "Nullable<Timestamp>"]
    pub cause_updated_at: Option<NaiveDateTime>,
    // Category fields
    #[sql_type = "Nullable<Text>"]
    pub category_name: Option<String>,
    #[sql_type = "Nullable<Uuid>"]
    pub category_id: Option<Uid>,
    #[sql_type = "Nullable<Timestamp>"]
    pub category_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub category_updated_at: Option<NaiveDateTime>,
    // Tag fields
    #[sql_type = "Nullable<Uuid>"]
    pub tag_id: Option<Uid>,
    #[sql_type = "Nullable<Text>"]
    pub tag_name: Option<String>,
    #[sql_type = "Nullable<Bool>"]
    pub tag_is_visible: Option<bool>,
    #[sql_type = "Nullable<Timestamp>"]
    pub tag_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub tag_updated_at: Option<NaiveDateTime>,
    // Impact fields
    #[sql_type = "Uuid"]
    pub impact_id: Uid,
    #[sql_type = "crate::chaos::sql_types::impact_status"]
    pub impact_status: ImpactStatus,
    #[sql_type = "Nullable<Uuid>"]
    pub impact_disruption_id: Option<Uid>,
    #[sql_type = "Timestamp"]
    pub impact_created_at: NaiveDateTime,
    #[sql_type = "Nullable<Timestamp>"]
    pub impact_updated_at: Option<NaiveDateTime>,
    // Application period fields
    #[sql_type = "Uuid"]
    pub application_id: Uid,
    #[sql_type = "Nullable<Timestamp>"]
    pub application_start_date: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub application_end_date: Option<NaiveDateTime>,
    // Severity fields
    #[sql_type = "Uuid"]
    pub severity_id: Uid,
    #[sql_type = "Text"]
    pub severity_wording: String,
    #[sql_type = "Nullable<Text>"]
    pub severity_color: Option<String>,
    #[sql_type = "Bool"]
    pub severity_is_visible: bool,
    #[sql_type = "Int4"]
    pub severity_priority: i32,
    #[sql_type = "Nullable<crate::chaos::sql_types::severity_effect>"]
    pub severity_effect: Option<SeverityEffect>,
    #[sql_type = "Timestamp"]
    pub severity_created_at: NaiveDateTime,
    #[sql_type = "Nullable<Timestamp>"]
    pub severity_updated_at: Option<NaiveDateTime>,
    // Ptobject fields
    #[sql_type = "Uuid"]
    pub ptobject_id: Uid,
    #[sql_type = "crate::chaos::sql_types::pt_object_type"]
    pub ptobject_type: PtObjectType,
    #[sql_type = "Nullable<Text>"]
    pub ptobject_uri: Option<String>,
    #[sql_type = "Timestamp"]
    pub ptobject_created_at: NaiveDateTime,
    #[sql_type = "Nullable<Timestamp>"]
    pub ptobject_updated_at: Option<NaiveDateTime>,
    // Ptobject line_section fields
    #[sql_type = "Nullable<Text>"]
    pub ls_line_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_line_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_line_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Text>"]
    pub ls_start_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_start_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_start_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Text>"]
    pub ls_end_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_end_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_end_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Uuid>"]
    pub ls_route_id: Option<Uid>,
    #[sql_type = "Nullable<Text>"]
    pub ls_route_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_route_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub ls_route_updated_at: Option<NaiveDateTime>,
    // Ptobject rail_section fields
    #[sql_type = "Nullable<Text>"]
    pub rs_line_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_line_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_line_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Text>"]
    pub rs_start_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_start_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_start_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Text>"]
    pub rs_end_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_end_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_end_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Uuid>"]
    pub rs_route_id: Option<Uid>,
    #[sql_type = "Nullable<Text>"]
    pub rs_route_uri: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_route_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub rs_route_updated_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Text>"]
    pub rs_blocked_sa: Option<String>,
    // Message fields
    #[sql_type = "Nullable<Uuid>"]
    pub message_id: Option<Uid>,
    #[sql_type = "Nullable<Text>"]
    pub message_text: Option<String>,
    #[sql_type = "Nullable<Timestamp>"]
    pub message_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub message_updated_at: Option<NaiveDateTime>,
    // Channel fields
    #[sql_type = "Nullable<Uuid>"]
    pub channel_id: Option<Uid>,
    #[sql_type = "Nullable<Text>"]
    pub channel_name: Option<String>,
    #[sql_type = "Nullable<Text>"]
    pub channel_content_type: Option<String>,
    #[sql_type = "Nullable<Int4>"]
    pub channel_max_size: Option<i32>,
    #[sql_type = "Nullable<Timestamp>"]
    pub channel_created_at: Option<NaiveDateTime>,
    #[sql_type = "Nullable<Timestamp>"]
    pub channel_updated_at: Option<NaiveDateTime>,
    // #[sql_type = "Nullable<Uuid>"]
    //  pub channel_type_id: Option<Uid>,
    #[sql_type = "Array<Nullable<crate::chaos::sql_types::channel_type_enum>>"]
    pub channel_type: Vec<Option<ChannelTypeSQL>>,
    //  Property & Associate property fields
    #[sql_type = "Nullable<Text>"]
    pub property_value: Option<String>,
    #[sql_type = "Nullable<Text>"]
    pub property_key: Option<String>,
    #[sql_type = "Nullable<Text>"]
    pub property_type: Option<String>,
    // Pattern & TimeSlot fields
    #[sql_type = "Nullable<Date>"]
    pub pattern_start_date: Option<NaiveDate>,
    #[sql_type = "Nullable<Date>"]
    pub pattern_end_date: Option<NaiveDate>,
    #[sql_type = "Nullable<Bit>"]
    pub pattern_weekly_pattern: Option<Vec<u8>>,
    #[sql_type = "Nullable<Uuid>"]
    pub pattern_id: Option<Uid>,
    #[sql_type = "Array<Nullable<Time>>"]
    pub time_slot_begin: Vec<Option<NaiveTime>>,
    #[sql_type = "Array<Nullable<Time>>"]
    pub time_slot_end: Vec<Option<NaiveTime>>,
    // #[sql_type = "Nullable<Uuid>"]
    //pub time_slot_id: Option<Uid>,
}
