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
* Pollen/air quality index (per hour, combined from [Buienradar] and
  [Luchtmeetnet])
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
...
   Compiling sinoptik v0.1.0 (/path/to/sinoptik)
    Finished release [optimized] target(s) in 9m 26s
     Running `/path/to/sinoptik/target/release/sinoptik`
```

(Note that Rocket listens on `127.0.0.1:3000` by default for debug builds, i.e.
builds when you don't add `--release`.)

You can provide Rocket with configuration to use a different address and/or port.
Just create a `Rocket.toml` file that contains (or copy `Rocket.toml.example`):

```toml
[default]
address = "0.0.0.0"
port = 2356
```

This will work independent of the type of build. For more about Rocket's
configuration, see: <https://rocket.rs/v0.5-rc/guide/configuration/>.

## Forecast API endpoint

The `/forecast` API endpoint provides forecasts per requested metric a list of
forecast item which are each comprised of a value and its (UNIX) timestamp. It
does so for a requested location.

### Locations

To select a location, you can either provide an address, or a geocoded position
by providing a latitude and longitude.
For example, to get forecasts for all metrics for the Stationsplein in Utrecht,
use:

```http
GET /forecast?address=Stationsplein,Utrecht&metrics[]=all
```

or directly by using its geocoded position:

```http
GET /forecast?lat=52.0902&lon=5.1114&metrics[]=all
```

### Metrics

When querying, the metrics need to be selected. It can be one of: `AQI`, `NO2`,
`O3`, `PAQI`, `PM10`, `pollen`, `precipitation` or `UVI`. If you use metric
`all`, or `all` is part of the selected metrics, all metrics will be retrieved.
Note that the parameter "array" notation as well as the repeated parameter
notation are supported. For example:

```http
GET /forecast?address=Stationsplein,Utrecht&metrics[]=AQI&metrics[]=pollen
GET /forecast?address=Stationsplein,Utrecht&metrics=AQI&metrics=pollen
GET /forecast?address=Stationsplein,Utrecht&metrics=all
```

### Forecast responses

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
  "lat": 52.0905169,
  "lon": 5.1109709,
  "time": 1652188682,
  "UVI": [
    {
      "time": 1652140800,
      "value": 4
    },
    {
      "time": 1652227200,
      "value": 4
    },
    {
      "time": 1652313600,
      "value": 4
    },
    {
      "time": 1652400000,
      "value": 4
    },
    {
      "time": 1652486400,
      "value": 5
    }
  ]
}
```

#### Combined metric PAQI

The PAQI (pollen/air quality index) metric is a special combined metric.
If selected, it merges items from the AQI and pollen metric into `PAQI` by
selecting the maximum value for each hour:

```json
{
  "lat": 52.0905169,
  "lon": 5.1109709,
  "time": 1652189065,
  "PAQI": [
    {
      "time": 1652187600,
      "value": 6.09
    },
    {
      "time": 1652191200,
      "value": 6.09
    },
    ...
  ]
}
```

#### Errors

If geocoding of an address is requested but fails, a not found error is
returned (HTTP 404). with the following body (this will change in the future):

```json
{
  "error": {
    "code": 404,
    "reason": "Not Found",
    "description": "The requested resource could not be found."
  }
}
```

If for any specific metric an error occurs, the list with forecast items will
be absent. However, the `errors` field will contain the error message for each
failed metric. For example, say Buienradar is down and precipitation forecast
items can not be retrieved:

```json
{
  "lat": 52.0905169,
  "lon": 5.1109709,
  "time": 1654524574,
  ...
  "errors": {
    "precipitation": "HTTP request error: error sending request for url (https://gpsgadget.buienradar.nl/data/raintext?lat=52.09&lon=5.11): error trying to connect: tcp connect error: Connection refused (os error 111)"
  }
}
```

## Map API endpoint

The `/map` API endpoint basically only exists for debugging purposes. Given an
address or geocoded position, it shows the current map for the provided metric
and draws a crosshair on the position.
Currently, only the `PAQI`, `pollen` and `UVI` metrics are backed by a map.

For example, to get the current pollen map with a crosshair on Stationsplein in
Utrecht, use:

```http
GET /map?address=Stationsplein,Utrecht&metric=pollen
```

or directly by using its geocoded position:

```http
GET /map?lat=52.0902&lon=5.1114&metric=pollen
```

### Map responses

The response is a PNG image with a crosshair drawn on the map. If geocoding of
an address fails or if the position is out of bounds of the map, nothing is
returned (HTTP 404). If the maps cannot/have not been downloaded or cached yet,
a service unavailable error is returned (HTTP 503).

## Version API endpoint

The `/version` API endpoint provides information of the current version and
build of the service.  This can be used to check if it needs to be updated.
Again, there is no path and no query parameters, just:

```http
GET /version
```

### Version responses

The response uses the JSON format and typically looks like this:

```json
{
  "version": "0.2.7",
  "timestamp": "2023-05-29T13:34:34.701323159Z",
  "git_sha": "bb5962d",
  "git_timestamp": "2023-05-29T15:32:17.000000000+02:00"
}
```

(Build and git information in example output may be out of date.)

## License

Sinoptik is licensed under the MIT license (see the `LICENSE` file or
<http://opensource.org/licenses/MIT>).
