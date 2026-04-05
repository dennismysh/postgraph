CREATE TABLE insights_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,
    report JSONB NOT NULL,
    context JSONB NOT NULL
);
