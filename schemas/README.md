# Schemas

- `celerrate-json-report.v1.schema.json`: the contract for
  `celerrate check --output=json`, authored here. Compatibility policy:
  adding a field is non-breaking and updates this file in the same
  release; removing a field or changing its meaning increments
  `schema_version` and forks a new file. The test suite validates real
  output against this file.
- `sarif-2.1.0.schema.json`: the official SARIF 2.1.0 schema, to be
  committed verbatim so the validation gate runs without network access.
  Provenance:
  https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json
  (mirror: https://json.schemastore.org/sarif-2.1.0.json).
