# Sinoptik

Sinoptik is a (REST) API service that provides an API for today's weather
forecast.  It can provide you with a specific set or all available metrics
that it supports.

Currently supported metrics are:

* Air quality index (per hour, from [Luchtmeetnet])
* NO₂ concentration (per hour, from [Luchtmeetnet])
* O₃ concentration (per hour, from [Luchtmeetnet])
* Particulate matter (PM10) concentration (per hour, from [Luchtmeetnet])
* Pollen (per hour, from [Buienradar])
* Pollen/air quality index (per hour, from [Buienradar])
* Precipitation (per 5 minutes, from [Buienradar])
* UV index (per day, from [Buienradar])

[Buienradar]: https://buienradar.nl
[Luchtmeetnet]: https://luchtmeetnet.nl

Because of the currently supported data providers, only data for
The Netherlands can be queried.

## Building & running

Using Cargo, it is easy to build and run Sinoptik, just run:

```shell
$ cargo run --release
```

(Note that Rocket listens on 127.0.0.1:3000 by default for debug builds, i.e. if you don't
add `--release`.)

You can provide Rocket with configuration to use a different address and/or port.
Just create a `Rocket.toml` file that contains (or copy `Rocket.toml.example`):

```toml
[default]
address = "0.0.0.0"
port = 4321
```

## Forecast API

The `/forecast` endpoint provides forecasts per requested metric a list of
forecast item which are each comprised of a value and its (UNIX) timestamp.
It does so for a requested location.

### Locations

To select a location, you can either provide an address, or a geocoded position
by providing a latitude and longitude.
For example to get forecasts for all metrics for the Stationsplein in Utrecht,
use:

```
GET /forecast?address=Stationsplein,Utrecht&metrics[]=all
```

or directly by using its geocoded position:


```
GET /forecast?lat=52.0902&lon=5.1114&metrics[]=all
```

### Metrics

When querying, the metrics need to be selected. It can be one of: `AQI`, `NO2`,
`O3`, `PAQI`, `PM10`, `pollen`, `precipitation` or `UVI`. If you use metric `all`, or
`all` is part of the selected metrics, all metrics will be retrieved.
Note that the parameter "array" as well as the repeated parameter notations are supported. For example:

```
GET /address=Stationsplein,Utrecht&metrics[]=AQI&metrics[]=pollen
GET /address=Stationsplein,Utrecht&metrics=AQI&metrics=pollen
GET /address=Stationsplein,Utrecht&metrics=all
```

### Response

The response of the API is a JSON object that contains three fixed fields:

* `lat`: the latitude of the geocoded position the forecast is for (number)
* `lon`: the longitude of the geocoded position the forecast is for (number)
* `time`: the (UNIX) timestamp of the forecast, basically "now" (number)

Then, it contains a field per requested metric with a list of forecast items
with two fixed fields as value:

* `time`: the (UNIX) timestamp for that forecasted value (number)
* `value`: the forecasted value for the metric (number)

An example when requesting just UVI (because it's short) for some random
position:

```json
{
  "lat": 34.567890,
  "lon": 1.234567,
  "time": 1645800043,
  "UVI": [
    {
      "time": 1645799526,
      "value": 1
    },
    {
      "time": 1645885926,
      "value": 2
    },
    {
      "time": 1645972326,
      "value": 3
    },
    {
      "time": 1646058726,
      "value": 2
    },
    {
      "time": 1646145126,
      "value": 1
    }
  ]
}
```

## License

Sinoptik is licensed under the MIT license (see the `LICENSE` file or
<http://opensource.org/licenses/MIT>).
