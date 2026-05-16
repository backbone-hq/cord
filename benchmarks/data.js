window.BENCHMARK_DATA = {
  "lastUpdate": 1778943404305,
  "repoUrl": "https://github.com/backbone-hq/cord",
  "entries": {
    "Rust Benchmark": [
      {
        "commit": {
          "author": {
            "email": "root@backbone.dev",
            "name": "Backbone Authors",
            "username": "backbone-root"
          },
          "committer": {
            "email": "root@backbone.dev",
            "name": "Backbone Authors",
            "username": "backbone-root"
          },
          "distinct": true,
          "id": "2c7f87c42bcc7e9da21ffea030e88b7e45cf9d15",
          "message": "Release v2.0.0-rc.1",
          "timestamp": "2026-05-16T14:53:29Z",
          "tree_id": "796ad18af7d03cef6d7dd969f8d4f83a9d69cc1f",
          "url": "https://github.com/backbone-hq/cord/commit/2c7f87c42bcc7e9da21ffea030e88b7e45cf9d15"
        },
        "date": 1778943403525,
        "tool": "cargo",
        "benches": [
          {
            "name": "Primitives/cord_ser_u64",
            "value": 13,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Primitives/bincode_ser_u64",
            "value": 14,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Primitives/cord_ser_str",
            "value": 27,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Primitives/bincode_ser_str",
            "value": 19,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Primitives/cord_de_str",
            "value": 33,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Collections/cord_ser_vec_100",
            "value": 283,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "Collections/bincode_ser_vec_100",
            "value": 78,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Collections/cord_ser_set_50",
            "value": 3849,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "Collections/cord_ser_map_50",
            "value": 4368,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "Complex/cord_ser_small_struct",
            "value": 29,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Complex/bincode_ser_small_struct",
            "value": 20,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "Complex/cord_ser_large_struct",
            "value": 1551,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "Complex/cord_de_large_struct",
            "value": 2942,
            "range": "± 43",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}