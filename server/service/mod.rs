/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

mod concept;
mod document;
mod error;
mod request_parser;
mod response_builders;
mod row;
pub(crate) mod transaction_service;
pub(crate) mod typedb_service;

pub(crate) type ConnectionID = uuid::Bytes;
