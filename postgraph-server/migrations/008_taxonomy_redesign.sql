-- Create intents table
CREATE TABLE intents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL
);

-- Create subjects table
CREATE TABLE subjects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL
);

-- Add intent_id and subject_id to posts
ALTER TABLE posts ADD COLUMN intent_id UUID REFERENCES intents(id) ON DELETE SET NULL;
ALTER TABLE posts ADD COLUMN subject_id UUID REFERENCES subjects(id) ON DELETE SET NULL;

-- Create subject_edges table
CREATE TABLE subject_edges (
    source_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    target_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    weight REAL NOT NULL,
    shared_intents INTEGER NOT NULL,
    PRIMARY KEY (source_subject_id, target_subject_id)
);
