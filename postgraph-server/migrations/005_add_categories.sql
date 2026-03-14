-- 005_add_categories.sql
CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    color TEXT
);

ALTER TABLE topics ADD COLUMN category_id UUID REFERENCES categories(id) ON DELETE SET NULL;
