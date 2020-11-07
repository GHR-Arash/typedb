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

package grakn.core.traversal;

import graql.lang.pattern.variable.Reference;

import javax.annotation.Nullable;
import java.util.Objects;

public abstract class Identifier {

    @Override
    public abstract boolean equals(Object o);

    @Override
    public abstract int hashCode();

    public static class Generated extends Identifier {

        private final int id;
        private final int hash;

        public Generated(int id) {
            this.id = id;
            this.hash = Objects.hash(Generated.class, id);
        }

        public static Generated of(int id) {
            return new Generated(id);
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            else if (o == null || getClass() != o.getClass()) return false;

            final Generated that = (Generated) o;
            return this.id == that.id;
        }

        @Override
        public int hashCode() {
            return hash;
        }
    }

    public abstract static class Variable extends Identifier {

        final Reference reference;
        private final Integer id;
        private final int hash;

        private Variable(Reference reference, @Nullable Integer id) {
            this.reference = reference;
            this.id = id;
            this.hash = Objects.hash(Variable.class, this.reference, this.id);
        }

        public static Referrable of(Reference.Referrable reference) {
            if (reference.isLabel()) return new Label(reference.asLabel());
            else if (reference.isName()) return new Name(reference.asName());
            else assert false;
            return null;
        }

        public static Anonymous of(Reference.Anonymous reference, int id) {
            return new Anonymous(reference, id);
        }

        public Reference reference() {
            return reference;
        }

        @Override
        public String toString() {
            return reference.syntax() + (id == null ? "" : id.toString());
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            else if (o == null || getClass() != o.getClass()) return false;

            final Variable that = (Variable) o;
            return this.reference.equals(that.reference) && Objects.equals(this.id, that.id);
        }

        @Override
        public int hashCode() {
            return hash;
        }

        static class Referrable extends Variable {

            Referrable(Reference reference) {
                super(reference, null);
            }

            @Override
            public Reference.Referrable reference() {
                return reference.asReferrable();
            }
        }

        static class Name extends Referrable {

            private Name(Reference.Name reference) {
                super(reference);
            }

            @Override
            public Reference.Name reference() {
                return reference.asName();
            }
        }

        static class Label extends Referrable {

            private Label(Reference.Label reference) {
                super(reference);
            }

            @Override
            public Reference.Label reference() {
                return reference.asLabel();
            }
        }

        static class Anonymous extends Variable {

            private Anonymous(Reference.Anonymous reference, int id) {
                super(reference, id);
            }

            @Override
            public Reference.Anonymous reference() {
                return reference.asAnonymous();
            }
        }
    }
}
