/*
 * Copyright (C) 2023 Vaticle
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::borrow::Cow;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Label {
    pub(crate) name: Cow<'static, str>,
    pub(crate) scope: Option<Cow<'static, str>>,
}

impl Label {
    pub fn name(&self) -> &str {
        &self.name
    }

    // TODO; can this just return an &Option<str> ?
    pub fn scope(&self) -> &Option<Cow<'static, str>> {
        &self.scope
    }
}
