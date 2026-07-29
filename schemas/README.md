# Schemas

- `celerrate-json-report.v1.schema.json`: the contract for
  `celerrate check --output=json`, authored here. Compatibility policy:
  adding a field is non-breaking and updates this file in the same
  release; removing a field or changing its meaning increments
  `schema_version` and forks a new file. The test suite validates real
  output against this file.
- `sarif-2.1.0.schema.json`: the official SARIF 2.1.0 schema, committed
  verbatim so the validation gate runs without network access.
  Provenance:
  https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json
  (mirror: https://json.schemastore.org/sarif-2.1.0.json). This schema
  is a normative component of the SARIF 2.1.0 Plus Errata 01 Work
  Product (the prose specification names it explicitly alongside the
  specification document itself), so it is covered by the same OASIS
  copyright and IPR terms: Copyright (C) OASIS Open 2023. All Rights
  Reserved. The document (and its Work Product components, including
  this schema) may be copied and furnished to others, and derivative
  works that comment on or explain it may be prepared, copied,
  published and distributed, in whole or in part, without restriction
  of any kind, provided this copyright notice is included on every
  copy; the document itself may not be modified. Full terms: the OASIS
  IPR Policy, https://www.oasis-open.org/policies-guidelines/ipr/.
