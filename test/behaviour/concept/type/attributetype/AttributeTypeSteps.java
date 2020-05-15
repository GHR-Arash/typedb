/*
 * Copyright (C) 2020 Grakn Labs
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
 *
 */

package hypergraph.test.behaviour.concept.type.attributetype;

import hypergraph.common.exception.HypergraphException;
import hypergraph.concept.type.AttributeType;
import hypergraph.concept.type.ThingType;
import io.cucumber.java.en.Then;
import io.cucumber.java.en.When;

import java.time.LocalDateTime;
import java.util.List;
import java.util.Set;

import static hypergraph.test.behaviour.connection.ConnectionSteps.tx;
import static java.util.stream.Collectors.toSet;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * Behaviour Steps specific to AttributeSteps
 */
public class AttributeTypeSteps {

    @When("put attribute type: {type_label}, value type: {value_type}")
    public void put_attribute_type_value_type(String typeLabel, Class<?> valueType) {
        tx().concepts().putAttributeType(typeLabel, valueType);
    }

    @Then("attribute\\( ?{type_label} ?) get value type: {value_type}")
    public void attribute_get_value_type(String typeLabel, Class<?> valueType) {
        assertEquals(valueType, tx().concepts().getAttributeType(typeLabel).valueType());
    }

    @Then("attribute\\( ?{type_label} ?) get supertype value type: {value_type}")
    public void attribute_get_supertype_value_type(String typeLabel, Class<?> valueType) {
        AttributeType supertype = tx().concepts().getAttributeType(typeLabel).sup();
        assertEquals(valueType, supertype.valueType());
    }

    @Then("attribute\\( ?{type_label} ?) fails at setting supertype: {type_label}")
    public void attribute_fails_at_setting_supertype(String typeLabel, String superLabel) {
        AttributeType superType = tx().concepts().getAttributeType(superLabel);
        try {
            tx().concepts().getAttributeType(typeLabel).sup(superType);
            fail();
        } catch (HypergraphException ignored) {
            assertTrue(true);
        }
    }

    private AttributeType attribute_as_value_type(String typeLabel, Class<?> valueType) {
        AttributeType attributeType = tx().concepts().getAttributeType(typeLabel);

        if (valueType.equals(Object.class)) {
            return attributeType.asObject();
        } else if (valueType.equals(Boolean.class)) {
            return attributeType.asBoolean();
        } else if (valueType.equals(Long.class)) {
            return attributeType.asLong();
        } else if (valueType.equals(Double.class)) {
            return attributeType.asDouble();
        } else if (valueType.equals(String.class)) {
            return attributeType.asString();
        } else if (valueType.equals(LocalDateTime.class)) {
            return attributeType.asDateTime();
        } else {
            throw new HypergraphException("unreachable");
        }
    }

    @Then("attribute\\( ?{type_label} ?) as\\( ?{value_type} ?) get subtypes contain:")
    public void attribute_as_get_subtypes_contain(String typeLabel, Class<?> valueType, List<String> subLabels) {
        AttributeType attributeType = attribute_as_value_type(typeLabel, valueType);
        Set<String> actuals = attributeType.subs().map(ThingType::label).collect(toSet());
        assertTrue(actuals.containsAll(subLabels));
    }

    @Then("attribute\\( ?{type_label} ?) as\\( ?{value_type} ?) get subtypes do not contain:")
    public void attribute_as_get_subtypes_do_not_contain(String typeLabel, Class<?> valueType, List<String> subLabels) {
        AttributeType attributeType = attribute_as_value_type(typeLabel, valueType);
        Set<String> actuals = attributeType.subs().map(ThingType::label).collect(toSet());
        for (String subLabel : subLabels) {
            assertFalse(actuals.contains(subLabel));
        }
    }
}
