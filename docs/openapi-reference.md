---
title: picasu v0.1.0
language_tabs:
  - shell: Shell
  - http: HTTP
  - javascript: JavaScript
  - ruby: Ruby
  - python: Python
  - php: PHP
  - java: Java
  - go: Go
toc_footers: []
includes: []
search: true
highlight_theme: darkula
headingLevel: 2
---

<!-- Generator: Widdershins v4.0.1 -->

<h1 id="picasu">picasu v0.1.0</h1>

> Scroll down for code samples, example requests and responses. Select a language for code samples from the tabs above or the mobile navigation menu.

License: MIT

<h1 id="picasu-default">Default</h1>

## delete_data

<a id="opIddelete_data"></a>

> Code samples

```shell
# You can also use wget
curl -X DELETE /delete/delete-data \
  -H 'Content-Type: application/json'

```

```http
DELETE /delete/delete-data HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "deleteList": [
    0
  ],
  "timestamp": 0
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/delete/delete-data',
{
  method: 'DELETE',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.delete '/delete/delete-data',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.delete('/delete/delete-data', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('DELETE','/delete/delete-data', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/delete/delete-data");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("DELETE");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("DELETE", "/delete/delete-data", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`DELETE /delete/delete-data`

> Body parameter

```json
{
  "deleteList": [0],
  "timestamp": 0
}
```

<h3 id="delete_data-parameters">Parameters</h3>

| Name | In   | Type                            | Required | Description |
| ---- | ---- | ------------------------------- | -------- | ----------- |
| body | body | [DeleteList](#schemadeletelist) | true     | none        |

<h3 id="delete_data-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Data deleted  | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## get_config_handler

<a id="opIdget_config_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/config \
  -H 'Accept: application/json'

```

```http
GET /get/config HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/config", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/config',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/config', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/config', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/config");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/config", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/config`

> Example responses

> 200 Response

```json
{
  "address": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "hasAuthKey": true,
  "hasPassword": true,
  "imagePath": "string",
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

<h3 id="get_config_handler-responses">Responses</h3>

| Status | Meaning                                                          | Description          | Schema                                  |
| ------ | ---------------------------------------------------------------- | -------------------- | --------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Public configuration | [ConfigResponse](#schemaconfigresponse) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input        | None                                    |

<aside class="success">
This operation does not require authentication
</aside>

## export_config_handler

<a id="opIdexport_config_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/config/export \
  -H 'Accept: text/plain'

```

```http
GET /get/config/export HTTP/1.1

Accept: text/plain

```

```javascript
const headers = {
  Accept: "text/plain",
};

fetch("/get/config/export", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'text/plain'
}

result = RestClient.get '/get/config/export',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'text/plain'
}

r = requests.get('/get/config/export', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'text/plain',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/config/export', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/config/export");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"text/plain"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/config/export", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/config/export`

> Example responses

> 200 Response

```
"string"
```

<h3 id="export_config_handler-responses">Responses</h3>

| Status | Meaning                                                          | Description            | Schema |
| ------ | ---------------------------------------------------------------- | ---------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Exported configuration | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input          | None   |

<aside class="success">
This operation does not require authentication
</aside>

## get_albums

<a id="opIdget_albums"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-albums \
  -H 'Accept: application/json'

```

```http
GET /get/get-albums HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-albums", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/get-albums',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-albums', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-albums', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-albums");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-albums", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-albums`

> Example responses

> 200 Response

```json
[
  {
    "albumId": "string",
    "albumName": "string",
    "dirPath": "string",
    "parentAlbumId": "string",
    "shareList": {
      "property1": {
        "description": "string",
        "exp": 0,
        "password": "string",
        "showDownload": true,
        "showMetadata": true,
        "showUpload": true,
        "url": "string"
      },
      "property2": {
        "description": "string",
        "exp": 0,
        "password": "string",
        "showDownload": true,
        "showMetadata": true,
        "showUpload": true,
        "url": "string"
      }
    }
  }
]
```

<h3 id="get_albums-responses">Responses</h3>

| Status | Meaning                                                 | Description    | Schema |
| ------ | ------------------------------------------------------- | -------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | List of albums | Inline |

<h3 id="get_albums-responseschema">Response Schema</h3>

Status Code **200**

| Name                        | Type                            | Required | Restrictions | Description                                                                                                       |
| --------------------------- | ------------------------------- | -------- | ------------ | ----------------------------------------------------------------------------------------------------------------- |
| _anonymous_                 | [[AlbumInfo](#schemaalbuminfo)] | false    | none         | none                                                                                                              |
| » albumId                   | string                          | true     | none         | none                                                                                                              |
| » albumName                 | string,null                     | false    | none         | none                                                                                                              |
| » dirPath                   | string,null                     | false    | none         | none                                                                                                              |
| » parentAlbumId             | string,null                     | false    | none         | Album ID of the direct parent directory album, or `None` for top-level<br>dir albums and all user-created albums. |
| » shareList                 | object                          | true     | none         | none                                                                                                              |
| »» **additionalProperties** | [Share](#schemashare)           | false    | none         | none                                                                                                              |
| »»» description             | string                          | true     | none         | none                                                                                                              |
| »»» exp                     | integer(int64)                  | true     | none         | none                                                                                                              |
| »»» password                | string,null                     | false    | none         | none                                                                                                              |
| »»» showDownload            | boolean                         | true     | none         | none                                                                                                              |
| »»» showMetadata            | boolean                         | true     | none         | none                                                                                                              |
| »»» showUpload              | boolean                         | true     | none         | none                                                                                                              |
| »»» url                     | string                          | true     | none         | none                                                                                                              |

<aside class="success">
This operation does not require authentication
</aside>

## get_data

<a id="opIdget_data"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-data \
  -H 'Accept: application/json'

```

```http
GET /get/get-data HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-data", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/get-data',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-data', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-data', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-data");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-data", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-data`

> Example responses

> 200 Response

```json
[
  {
    "abstractData": {},
    "timestamp": 0,
    "token": "string"
  }
]
```

<h3 id="get_data-responses">Responses</h3>

| Status | Meaning                                                          | Description             | Schema |
| ------ | ---------------------------------------------------------------- | ----------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Data by timestamp range | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input           | None   |

<h3 id="get_data-responseschema">Response Schema</h3>

Status Code **200**

| Name           | Type                                                        | Required | Restrictions | Description |
| -------------- | ----------------------------------------------------------- | -------- | ------------ | ----------- |
| _anonymous_    | [[DataBaseTimestampReturn](#schemadatabasetimestampreturn)] | false    | none         | none        |
| » abstractData | object                                                      | true     | none         | none        |
| » timestamp    | integer(int64)                                              | true     | none         | none        |
| » token        | string                                                      | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## get_export

<a id="opIdget_export"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-export

```

```http
GET /get/get-export HTTP/1.1

```

```javascript
fetch("/get/get-export", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/get/get-export',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/get/get-export')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-export', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-export");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-export", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-export`

<h3 id="get_export-responses">Responses</h3>

| Status | Meaning                                                          | Description         | Schema |
| ------ | ---------------------------------------------------------------- | ------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Export data as JSON | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## get_rows

<a id="opIdget_rows"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-rows \
  -H 'Accept: application/json'

```

```http
GET /get/get-rows HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-rows", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/get-rows',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-rows', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-rows', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-rows");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-rows", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-rows`

> Example responses

> 200 Response

```json
{
  "displayElements": [
    {
      "displayHeight": 0,
      "displayWidth": 0
    }
  ],
  "end": 0,
  "rowIndex": 0,
  "start": 0
}
```

<h3 id="get_rows-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema            |
| ------ | ---------------------------------------------------------------- | ------------- | ----------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Row data      | [Row](#schemarow) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None              |

<aside class="success">
This operation does not require authentication
</aside>

## get_scroll_bar

<a id="opIdget_scroll_bar"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-scroll-bar \
  -H 'Accept: application/json'

```

```http
GET /get/get-scroll-bar HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-scroll-bar", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/get-scroll-bar',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-scroll-bar', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-scroll-bar', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-scroll-bar");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-scroll-bar", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-scroll-bar`

> Example responses

> 200 Response

```json
[
  {
    "index": 0,
    "month": 0,
    "year": 0
  }
]
```

<h3 id="get_scroll_bar-responses">Responses</h3>

| Status | Meaning                                                          | Description     | Schema |
| ------ | ---------------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Scroll bar data | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input   | None   |

<h3 id="get_scroll_bar-responseschema">Response Schema</h3>

Status Code **200**

| Name        | Type                                    | Required | Restrictions | Description |
| ----------- | --------------------------------------- | -------- | ------------ | ----------- |
| _anonymous_ | [[ScrollBarData](#schemascrollbardata)] | false    | none         | none        |
| » index     | integer                                 | true     | none         | none        |
| » month     | integer                                 | true     | none         | none        |
| » year      | integer                                 | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## get_tags

<a id="opIdget_tags"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-tags \
  -H 'Accept: application/json'

```

```http
GET /get/get-tags HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-tags", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/get-tags',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-tags', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/get-tags', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/get-tags");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/get-tags", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/get-tags`

> Example responses

> 200 Response

```json
[
  {
    "number": 0,
    "tag": "string"
  }
]
```

<h3 id="get_tags-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | List of tags  | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<h3 id="get_tags-responseschema">Response Schema</h3>

Status Code **200**

| Name        | Type                        | Required | Restrictions | Description |
| ----------- | --------------------------- | -------- | ------------ | ----------- |
| _anonymous_ | [[TagInfo](#schemataginfo)] | false    | none         | none        |
| » number    | integer                     | true     | none         | none        |
| » tag       | string                      | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## get_album_index_status

<a id="opIdget_album_index_status"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/index/status \
  -H 'Accept: application/json'

```

```http
GET /get/index/status HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/index/status", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/index/status',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/index/status', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/index/status', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/index/status");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/index/status", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/index/status`

> Example responses

> 200 Response

```json
{
  "cancelRequested": true,
  "failed": 0,
  "finishedAt": 0,
  "matched": 0,
  "processed": 0,
  "root": "string",
  "scanned": 0,
  "startedAt": 0,
  "state": "idle"
}
```

<h3 id="get_album_index_status-responses">Responses</h3>

| Status | Meaning                                                          | Description        | Schema                                      |
| ------ | ---------------------------------------------------------------- | ------------------ | ------------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album index status | [AlbumIndexStatus](#schemaalbumindexstatus) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input      | None                                        |

<aside class="success">
This operation does not require authentication
</aside>

## get_fs_completion

<a id="opIdget_fs_completion"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/path-completion \
  -H 'Accept: application/json'

```

```http
GET /get/path-completion HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/path-completion", {
  method: "GET",

  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Accept' => 'application/json'
}

result = RestClient.get '/get/path-completion',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/path-completion', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/path-completion', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/path-completion");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/get/path-completion", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/path-completion`

> Example responses

> 200 Response

```json
{
  "children": ["string"],
  "is_default": true,
  "roots": ["string"]
}
```

<h3 id="get_fs_completion-responses">Responses</h3>

| Status | Meaning                                                          | Description                | Schema                              |
| ------ | ---------------------------------------------------------------- | -------------------------- | ----------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Filesystem path completion | [FsCompletion](#schemafscompletion) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input              | None                                |

<aside class="success">
This operation does not require authentication
</aside>

## prefetch

<a id="opIdprefetch"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /get/prefetch \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json'

```

```http
POST /get/prefetch HTTP/1.1

Content-Type: application/json
Accept: application/json

```

```javascript
const inputBody = "null";
const headers = {
  "Content-Type": "application/json",
  Accept: "application/json",
};

fetch("/get/prefetch", {
  method: "POST",
  body: inputBody,
  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json',
  'Accept' => 'application/json'
}

result = RestClient.post '/get/prefetch',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json',
  'Accept': 'application/json'
}

r = requests.post('/get/prefetch', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/get/prefetch', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/get/prefetch");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/get/prefetch", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /get/prefetch`

> Body parameter

```json
null
```

<h3 id="prefetch-parameters">Parameters</h3>

| Name | In   | Type | Required | Description |
| ---- | ---- | ---- | -------- | ----------- |
| body | body | any  | true     | none        |

> Example responses

> 200 Response

```json
{
  "prefetch": {
    "dataLength": 0,
    "locateTo": 0,
    "timestamp": 0
  },
  "resolvedShareOpt": {},
  "token": "string"
}
```

<h3 id="prefetch-responses">Responses</h3>

| Status | Meaning                                                          | Description     | Schema                                  |
| ------ | ---------------------------------------------------------------- | --------------- | --------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Prefetch result | [PrefetchReturn](#schemaprefetchreturn) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input   | None                                    |

<aside class="success">
This operation does not require authentication
</aside>

## compressed_file

<a id="opIdcompressed_file"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /object/compressed/{file_path}

```

```http
GET /object/compressed/{file_path} HTTP/1.1

```

```javascript
fetch("/object/compressed/{file_path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/object/compressed/{file_path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/object/compressed/{file_path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/object/compressed/{file_path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/object/compressed/{file_path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/object/compressed/{file_path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /object/compressed/{file_path}`

<h3 id="compressed_file-responses">Responses</h3>

| Status | Meaning                                                          | Description     | Schema |
| ------ | ---------------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Compressed file | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input   | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the original file directly from its current location under

`imagePath` — there is no copy of it under `DATA_HOME`; `IMAGE_HOME` is
the single, authoritative copy (see `docs/design.md` "Albums" and
`TODO.md`'s "Storage architecture fix"). The route's `<file_path..>`
segment is still `<hash-prefix>/<hash>.<ext>` for URL compatibility with
the frontend and `GuardHashOriginal`'s validation, but only the hash
(the file stem) is actually used, to look up the record's current
`source_path()`.

<a id="opIdimported_file"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /object/imported/{file_path}

```

```http
GET /object/imported/{file_path} HTTP/1.1

```

```javascript
fetch("/object/imported/{file_path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/object/imported/{file_path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/object/imported/{file_path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/object/imported/{file_path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/object/imported/{file_path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/object/imported/{file_path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /object/imported/{file_path}`

<h3 id="serve-the-original-file-directly-from-its-current-location-under
`imagepath`-—-there-is-no-copy-of-it-under-`data_home`;-`image_home`-is
the-single,-authoritative-copy-(see-`docs/design.md`-"albums"-and
`todo.md`'s-"storage-architecture-fix").-the-route's-`<file_path..>`
segment-is-still-`<hash-prefix>/<hash>.<ext>`-for-url-compatibility-with
the-frontend-and-`guardhashoriginal`'s-validation,-but-only-the-hash
(the-file-stem)-is-actually-used,-to-look-up-the-record's-current
`source_path()`.-responses">Responses</h3>

| Status | Meaning                                                          | Description            | Schema |
| ------ | ---------------------------------------------------------------- | ---------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Imported original file | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input          | None   |

<aside class="success">
This operation does not require authentication
</aside>

## authenticate

<a id="opIdauthenticate"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/authenticate \
  -H 'Content-Type: text/plain' \
  -H 'Accept: text/plain'

```

```http
POST /post/authenticate HTTP/1.1

Content-Type: text/plain
Accept: text/plain

```

```javascript
const inputBody = "string";
const headers = {
  "Content-Type": "text/plain",
  Accept: "text/plain",
};

fetch("/post/authenticate", {
  method: "POST",
  body: inputBody,
  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'text/plain',
  'Accept' => 'text/plain'
}

result = RestClient.post '/post/authenticate',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'text/plain',
  'Accept': 'text/plain'
}

r = requests.post('/post/authenticate', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'text/plain',
    'Accept' => 'text/plain',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/authenticate', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/authenticate");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"text/plain"},
        "Accept": []string{"text/plain"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/authenticate", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/authenticate`

> Body parameter

```
string

```

<h3 id="authenticate-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description |
| ---- | ---- | ------ | -------- | ----------- |
| body | body | string | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="authenticate-responses">Responses</h3>

| Status | Meaning                                                         | Description      | Schema |
| ------ | --------------------------------------------------------------- | ---------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)         | JWT token        | string |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Invalid password | None   |

<aside class="success">
This operation does not require authentication
</aside>

## import_config_handler

<a id="opIdimport_config_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/config/import \
  -H 'Content-Type: application/json'

```

```http
POST /post/config/import HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "address": "string",
  "authKey": "string",
  "dataHome": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "imagePath": "string",
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "password": "string",
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/post/config/import',
{
  method: 'POST',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.post '/post/config/import',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.post('/post/config/import', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/config/import', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/config/import");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/config/import", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/config/import`

> Body parameter

```json
{
  "address": "string",
  "authKey": "string",
  "dataHome": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "imagePath": "string",
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "password": "string",
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

<h3 id="import_config_handler-parameters">Parameters</h3>

| Name | In   | Type                          | Required | Description |
| ---- | ---- | ----------------------------- | -------- | ----------- |
| body | body | [AppConfig](#schemaappconfig) | true     | none        |

<h3 id="import_config_handler-responses">Responses</h3>

| Status | Meaning                                                          | Description     | Schema |
| ------ | ---------------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Config imported | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input   | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Create a new subdirectory under an existing dir-album's directory and

register it as a new album. Returns the new album's ID.

<a id="opIdcreate_dir_album"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/create_dir_album \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/plain'

```

```http
POST /post/create_dir_album HTTP/1.1

Content-Type: application/json
Accept: text/plain

```

```javascript
const inputBody = '{
  "name": "string",
  "parentAlbumId": "string"
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'text/plain'
};

fetch('/post/create_dir_album',
{
  method: 'POST',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json',
  'Accept' => 'text/plain'
}

result = RestClient.post '/post/create_dir_album',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json',
  'Accept': 'text/plain'
}

r = requests.post('/post/create_dir_album', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
    'Accept' => 'text/plain',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/create_dir_album', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/create_dir_album");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
        "Accept": []string{"text/plain"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/create_dir_album", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/create_dir_album`

> Body parameter

```json
{
  "name": "string",
  "parentAlbumId": "string"
}
```

<h3 id="create-a-new-subdirectory-under-an-existing-dir-album's-directory-and
register-it-as-a-new-album.-returns-the-new-album's-id.-parameters">Parameters</h3>

| Name | In   | Type                                            | Required | Description |
| ---- | ---- | ----------------------------------------------- | -------- | ----------- |
| body | body | [CreateDirAlbumData](#schemacreatediralbumdata) | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="create-a-new-subdirectory-under-an-existing-dir-album's-directory-and
register-it-as-a-new-album.-returns-the-new-album's-id.-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | New album ID  | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## create_share

<a id="opIdcreate_share"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/create_share \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/plain'

```

```http
POST /post/create_share HTTP/1.1

Content-Type: application/json
Accept: text/plain

```

```javascript
const inputBody = '{
  "albumId": "string",
  "description": "string",
  "exp": 0,
  "password": "string",
  "showDownload": true,
  "showMetadata": true,
  "showUpload": true
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'text/plain'
};

fetch('/post/create_share',
{
  method: 'POST',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json',
  'Accept' => 'text/plain'
}

result = RestClient.post '/post/create_share',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json',
  'Accept': 'text/plain'
}

r = requests.post('/post/create_share', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
    'Accept' => 'text/plain',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/create_share', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/create_share");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
        "Accept": []string{"text/plain"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/create_share", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/create_share`

> Body parameter

```json
{
  "albumId": "string",
  "description": "string",
  "exp": 0,
  "password": "string",
  "showDownload": true,
  "showMetadata": true,
  "showUpload": true
}
```

<h3 id="create_share-parameters">Parameters</h3>

| Name | In   | Type                              | Required | Description |
| ---- | ---- | --------------------------------- | -------- | ----------- |
| body | body | [CreateShare](#schemacreateshare) | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="create_share-responses">Responses</h3>

| Status | Meaning                                                          | Description        | Schema |
| ------ | ---------------------------------------------------------------- | ------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Share link created | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input      | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Walk a directory under `IMAGE_HOME` and index all media files in the

background. `album` is a path relative to `IMAGE_HOME` — use `"/"` for
the root. Status can be polled via `GET /get/index/status`.

<a id="opIdindex_album_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/index/album \
  -H 'Content-Type: application/json'

```

```http
POST /post/index/album HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "album": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/post/index/album',
{
  method: 'POST',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.post '/post/index/album',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.post('/post/index/album', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/index/album', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/index/album");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/index/album", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/index/album`

> Body parameter

```json
{
  "album": "string"
}
```

<h3 id="walk-a-directory-under-`image_home`-and-index-all-media-files-in-the
background.--`album`-is-a-path-relative-to-`image_home`-—-use-`"/"`-for
the-root.--status-can-be-polled-via-`get-/get/index/status`.-parameters">Parameters</h3>

| Name | In   | Type                                          | Required | Description |
| ---- | ---- | --------------------------------------------- | -------- | ----------- |
| body | body | [IndexAlbumRequest](#schemaindexalbumrequest) | true     | none        |

<h3 id="walk-a-directory-under-`image_home`-and-index-all-media-files-in-the
background.--`album`-is-a-path-relative-to-`image_home`-—-use-`"/"`-for
the-root.--status-can-be-polled-via-`get-/get/index/status`.-responses">Responses</h3>

| Status | Meaning                                                          | Description            | Schema |
| ------ | ---------------------------------------------------------------- | ---------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album indexing started | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input          | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Cancel a running album index job.

<a id="opIdcancel_album_index_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/index/cancel

```

```http
POST /post/index/cancel HTTP/1.1

```

```javascript
fetch("/post/index/cancel", {
  method: "POST",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.post '/post/index/cancel',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.post('/post/index/cancel')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/index/cancel', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/index/cancel");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/index/cancel", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/index/cancel`

<h3 id="cancel-a-running-album-index-job.-responses">Responses</h3>

| Status | Meaning                                                          | Description           | Schema |
| ------ | ---------------------------------------------------------------- | --------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album index cancelled | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input         | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Index a single image by its path relative to `IMAGE_HOME`. Runs in the

background; returns `202 Accepted` immediately.

<a id="opIdindex_image_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/index/image \
  -H 'Content-Type: application/json'

```

```http
POST /post/index/image HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "album": "string",
  "image": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/post/index/image',
{
  method: 'POST',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.post '/post/index/image',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.post('/post/index/image', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/post/index/image', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/post/index/image");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/index/image", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/index/image`

> Body parameter

```json
{
  "album": "string",
  "image": "string"
}
```

<h3 id="index-a-single-image-by-its-path-relative-to-`image_home`.--runs-in-the
background;-returns-`202-accepted`-immediately.-parameters">Parameters</h3>

| Name | In   | Type                                          | Required | Description |
| ---- | ---- | --------------------------------------------- | -------- | ----------- |
| body | body | [IndexImageRequest](#schemaindeximagerequest) | true     | none        |

<h3 id="index-a-single-image-by-its-path-relative-to-`image_home`.--runs-in-the
background;-returns-`202-accepted`-immediately.-responses">Responses</h3>

| Status | Meaning                                                          | Description            | Schema |
| ------ | ---------------------------------------------------------------- | ---------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Image indexing started | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input          | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Move a media item into the album's directory on disk, update the DB alias,

and record the explicit album membership. Returns 400 if the file is not
found at the recorded alias path (stale alias — user must re-index first).

<a id="opIdassign_album"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/assign_album \
  -H 'Content-Type: application/json'

```

```http
PUT /put/assign_album HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "hash": "string",
  "onConflict": "skip"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/assign_album',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/assign_album',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/assign_album', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/assign_album', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/assign_album");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/assign_album", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/assign_album`

> Body parameter

```json
{
  "albumId": "string",
  "hash": "string",
  "onConflict": "skip"
}
```

<h3 id="move-a-media-item-into-the-album's-directory-on-disk,-update-the-db-alias,
and-record-the-explicit-album-membership.--returns-400-if-the-file-is-not
found-at-the-recorded-alias-path-(stale-alias-—-user-must-re-index-first).-parameters">Parameters</h3>

| Name | In   | Type                                      | Required | Description |
| ---- | ---- | ----------------------------------------- | -------- | ----------- |
| body | body | [AssignAlbumData](#schemaassignalbumdata) | true     | none        |

<h3 id="move-a-media-item-into-the-album's-directory-on-disk,-update-the-db-alias,
and-record-the-explicit-album-membership.--returns-400-if-the-file-is-not
found-at-the-recorded-alias-path-(stale-alias-—-user-must-re-index-first).-responses">Responses</h3>

| Status | Meaning                                                          | Description                     | Schema |
| ------ | ---------------------------------------------------------------- | ------------------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Item assigned to album          | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input or item not found | None   |

<aside class="success">
This operation does not require authentication
</aside>

## update_config_handler

<a id="opIdupdate_config_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/config \
  -H 'Content-Type: application/json'

```

```http
PUT /put/config HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "address": "string",
  "authKey": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/config',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/config',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/config', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/config', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/config");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/config", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/config`

> Body parameter

```json
{
  "address": "string",
  "authKey": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

<h3 id="update_config_handler-parameters">Parameters</h3>

| Name | In   | Type                                                            | Required | Description |
| ---- | ---- | --------------------------------------------------------------- | -------- | ----------- |
| body | body | [PartialUpdateConfigRequest](#schemapartialupdateconfigrequest) | true     | none        |

<h3 id="update_config_handler-responses">Responses</h3>

| Status | Meaning                                                          | Description    | Schema |
| ------ | ---------------------------------------------------------------- | -------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Config updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input  | None   |

<aside class="success">
This operation does not require authentication
</aside>

## update_password_handler

<a id="opIdupdate_password_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/config/password \
  -H 'Content-Type: application/json'

```

```http
PUT /put/config/password HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "oldPassword": "string",
  "password": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/config/password',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/config/password',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/config/password', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/config/password', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/config/password");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/config/password", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/config/password`

> Body parameter

```json
{
  "oldPassword": "string",
  "password": "string"
}
```

<h3 id="update_password_handler-parameters">Parameters</h3>

| Name | In   | Type                                                  | Required | Description |
| ---- | ---- | ----------------------------------------------------- | -------- | ----------- |
| body | body | [UpdatePasswordRequest](#schemaupdatepasswordrequest) | true     | none        |

<h3 id="update_password_handler-responses">Responses</h3>

| Status | Meaning                                                          | Description      | Schema |
| ------ | ---------------------------------------------------------------- | ---------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Password updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input    | None   |

<aside class="success">
This operation does not require authentication
</aside>

## delete_share

<a id="opIddelete_share"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/delete_share \
  -H 'Content-Type: application/json'

```

```http
PUT /put/delete_share HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "shareId": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/delete_share',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/delete_share',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/delete_share', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/delete_share', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/delete_share");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/delete_share", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/delete_share`

> Body parameter

```json
{
  "albumId": "string",
  "shareId": "string"
}
```

<h3 id="delete_share-parameters">Parameters</h3>

| Name | In   | Type                              | Required | Description |
| ---- | ---- | --------------------------------- | -------- | ----------- |
| body | body | [DeleteShare](#schemadeleteshare) | true     | none        |

<h3 id="delete_share-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Share deleted | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## edit_flags

<a id="opIdedit_flags"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/edit_flags \
  -H 'Content-Type: application/json'

```

```http
PUT /put/edit_flags HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "indexArray": [
    0
  ],
  "isArchived": true,
  "isFavorite": true,
  "isTrashed": true,
  "timestamp": 0
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/edit_flags',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/edit_flags',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/edit_flags', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/edit_flags', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/edit_flags");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/edit_flags", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/edit_flags`

> Body parameter

```json
{
  "indexArray": [0],
  "isArchived": true,
  "isFavorite": true,
  "isTrashed": true,
  "timestamp": 0
}
```

<h3 id="edit_flags-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [EditFlagsData](#schemaeditflagsdata) | true     | none        |

<h3 id="edit_flags-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Flags updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## edit_rating

<a id="opIdedit_rating"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/edit_rating \
  -H 'Content-Type: application/json'

```

```http
PUT /put/edit_rating HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "indexArray": [
    0
  ],
  "rating": 0,
  "timestamp": 0
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/edit_rating',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/edit_rating',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/edit_rating', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/edit_rating', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/edit_rating");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/edit_rating", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/edit_rating`

> Body parameter

```json
{
  "indexArray": [0],
  "rating": 0,
  "timestamp": 0
}
```

<h3 id="edit_rating-parameters">Parameters</h3>

| Name | In   | Type                                    | Required | Description |
| ---- | ---- | --------------------------------------- | -------- | ----------- |
| body | body | [EditRatingData](#schemaeditratingdata) | true     | none        |

<h3 id="edit_rating-responses">Responses</h3>

| Status | Meaning                                                          | Description    | Schema |
| ------ | ---------------------------------------------------------------- | -------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Rating updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input  | None   |

<aside class="success">
This operation does not require authentication
</aside>

## edit_share

<a id="opIdedit_share"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/edit_share \
  -H 'Content-Type: application/json'

```

```http
PUT /put/edit_share HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "share": {
    "description": "string",
    "exp": 0,
    "password": "string",
    "showDownload": true,
    "showMetadata": true,
    "showUpload": true,
    "url": "string"
  }
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/edit_share',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/edit_share',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/edit_share', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/edit_share', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/edit_share");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/edit_share", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/edit_share`

> Body parameter

```json
{
  "albumId": "string",
  "share": {
    "description": "string",
    "exp": 0,
    "password": "string",
    "showDownload": true,
    "showMetadata": true,
    "showUpload": true,
    "url": "string"
  }
}
```

<h3 id="edit_share-parameters">Parameters</h3>

| Name | In   | Type                          | Required | Description |
| ---- | ---- | ----------------------------- | -------- | ----------- |
| body | body | [EditShare](#schemaeditshare) | true     | none        |

<h3 id="edit_share-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Share updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## edit_tag

<a id="opIdedit_tag"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/edit_tag \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json'

```

```http
PUT /put/edit_tag HTTP/1.1

Content-Type: application/json
Accept: application/json

```

```javascript
const inputBody = '{
  "addTagsArray": [
    "string"
  ],
  "indexArray": [
    0
  ],
  "removeTagsArray": [
    "string"
  ],
  "timestamp": 0
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'application/json'
};

fetch('/put/edit_tag',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json',
  'Accept' => 'application/json'
}

result = RestClient.put '/put/edit_tag',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json',
  'Accept': 'application/json'
}

r = requests.put('/put/edit_tag', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
    'Accept' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/edit_tag', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/edit_tag");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/edit_tag", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/edit_tag`

> Body parameter

```json
{
  "addTagsArray": ["string"],
  "indexArray": [0],
  "removeTagsArray": ["string"],
  "timestamp": 0
}
```

<h3 id="edit_tag-parameters">Parameters</h3>

| Name | In   | Type                                | Required | Description |
| ---- | ---- | ----------------------------------- | -------- | ----------- |
| body | body | [EditTagsData](#schemaedittagsdata) | true     | none        |

> Example responses

> 200 Response

```json
[
  {
    "number": 0,
    "tag": "string"
  }
]
```

<h3 id="edit_tag-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Tags updated  | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<h3 id="edit_tag-responseschema">Response Schema</h3>

Status Code **200**

| Name        | Type                        | Required | Restrictions | Description |
| ----------- | --------------------------- | -------- | ------------ | ----------- |
| _anonymous_ | [[TagInfo](#schemataginfo)] | false    | none         | none        |
| » number    | integer                     | true     | none         | none        |
| » tag       | string                      | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## regenerate_thumbnail_with_frame

<a id="opIdregenerate_thumbnail_with_frame"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/regenerate-thumbnail-with-frame \
  -H 'Content-Type: application/json'

```

```http
PUT /put/regenerate-thumbnail-with-frame HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = "null";
const headers = {
  "Content-Type": "application/json",
};

fetch("/put/regenerate-thumbnail-with-frame", {
  method: "PUT",
  body: inputBody,
  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/regenerate-thumbnail-with-frame',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/regenerate-thumbnail-with-frame', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/regenerate-thumbnail-with-frame', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/regenerate-thumbnail-with-frame");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/regenerate-thumbnail-with-frame", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/regenerate-thumbnail-with-frame`

> Body parameter

```json
null
```

<h3 id="regenerate_thumbnail_with_frame-parameters">Parameters</h3>

| Name | In   | Type | Required | Description |
| ---- | ---- | ---- | -------- | ----------- |
| body | body | any  | true     | none        |

<h3 id="regenerate_thumbnail_with_frame-responses">Responses</h3>

| Status | Meaning                                                          | Description           | Schema |
| ------ | ---------------------------------------------------------------- | --------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Thumbnail regenerated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input         | None   |

<aside class="success">
This operation does not require authentication
</aside>

## rotate_image

<a id="opIdrotate_image"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/rotate-image \
  -H 'Content-Type: application/json'

```

```http
PUT /put/rotate-image HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "hash": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/rotate-image',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/rotate-image',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/rotate-image', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/rotate-image', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/rotate-image");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/rotate-image", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/rotate-image`

> Body parameter

```json
{
  "hash": "string"
}
```

<h3 id="rotate_image-parameters">Parameters</h3>

| Name | In   | Type                                            | Required | Description |
| ---- | ---- | ----------------------------------------------- | -------- | ----------- |
| body | body | [RotateImageRequest](#schemarotateimagerequest) | true     | none        |

<h3 id="rotate_image-responses">Responses</h3>

| Status | Meaning                                                          | Description   | Schema |
| ------ | ---------------------------------------------------------------- | ------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Image rotated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Updates the cover image of a specific album.

<a id="opIdset_album_cover"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/set_album_cover \
  -H 'Content-Type: application/json'

```

```http
PUT /put/set_album_cover HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "coverHash": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/set_album_cover',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/set_album_cover',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/set_album_cover', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/set_album_cover', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/set_album_cover");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/set_album_cover", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/set_album_cover`

> Body parameter

```json
{
  "albumId": "string",
  "coverHash": "string"
}
```

<h3 id="updates-the-cover-image-of-a-specific-album.-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [SetAlbumCover](#schemasetalbumcover) | true     | none        |

<h3 id="updates-the-cover-image-of-a-specific-album.-responses">Responses</h3>

| Status | Meaning                                                          | Description         | Schema |
| ------ | ---------------------------------------------------------------- | ------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album cover updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Updates the display title of a specific album.

<a id="opIdset_album_title"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/set_album_title \
  -H 'Content-Type: application/json'

```

```http
PUT /put/set_album_title HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "title": "string"
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/set_album_title',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/set_album_title',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/set_album_title', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/set_album_title', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/set_album_title");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/set_album_title", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/set_album_title`

Sets both the raw display `title` and `custom_title` (the value actually
persisted to `.albuminfo.xmp` by `write_sidecar_for`). Clearing the title
(`title: None`) falls back `title` to the directory-derived default for
dir-albums, so the sidecar-freezing bug can't reappear via this path.

> Body parameter

```json
{
  "albumId": "string",
  "title": "string"
}
```

<h3 id="updates-the-display-title-of-a-specific-album.-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [SetAlbumTitle](#schemasetalbumtitle) | true     | none        |

<h3 id="updates-the-display-title-of-a-specific-album.-responses">Responses</h3>

| Status | Meaning                                                          | Description         | Schema |
| ------ | ---------------------------------------------------------------- | ------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album title updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## set_user_defined_description

<a id="opIdset_user_defined_description"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/set_user_defined_description \
  -H 'Content-Type: application/json'

```

```http
PUT /put/set_user_defined_description HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = '{
  "description": "string",
  "index": 0,
  "timestamp": 0
}';
const headers = {
  'Content-Type':'application/json'
};

fetch('/put/set_user_defined_description',
{
  method: 'PUT',
  body: inputBody,
  headers: headers
})
.then(function(res) {
    return res.json();
}).then(function(body) {
    console.log(body);
});

```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.put '/put/set_user_defined_description',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.put('/put/set_user_defined_description', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('PUT','/put/set_user_defined_description', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/put/set_user_defined_description");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("PUT");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("PUT", "/put/set_user_defined_description", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`PUT /put/set_user_defined_description`

> Body parameter

```json
{
  "description": "string",
  "index": 0,
  "timestamp": 0
}
```

<h3 id="set_user_defined_description-parameters">Parameters</h3>

| Name | In   | Type                                                          | Required | Description |
| ---- | ---- | ------------------------------------------------------------- | -------- | ----------- |
| body | body | [SetUserDefinedDescription](#schemasetuserdefineddescription) | true     | none        |

<h3 id="set_user_defined_description-responses">Responses</h3>

| Status | Meaning                                                          | Description         | Schema |
| ------ | ---------------------------------------------------------------- | ------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Description updated | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## upload

<a id="opIdupload"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /upload \
  -H 'Content-Type: application/json'

```

```http
POST /upload HTTP/1.1

Content-Type: application/json

```

```javascript
const inputBody = "null";
const headers = {
  "Content-Type": "application/json",
};

fetch("/upload", {
  method: "POST",
  body: inputBody,
  headers: headers,
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

headers = {
  'Content-Type' => 'application/json'
}

result = RestClient.post '/upload',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'application/json'
}

r = requests.post('/upload', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'application/json',
);

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('POST','/upload', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/upload");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("POST");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    headers := map[string][]string{
        "Content-Type": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/upload", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /upload`

> Body parameter

```json
null
```

<h3 id="upload-parameters">Parameters</h3>

| Name        | In    | Type    | Required | Description                                                                                                                                                                                                                                                                                                                                                                       |
| ----------- | ----- | ------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| auto_rename | query | boolean | false    | When true (the default), uploaded filenames are sanitized automatically: forbidden characters are stripped, reserved Windows names are prefixed, and Unicode NFC normalization is applied; a name that degrades to empty falls back to 'upload', yielding an 'upload-{uuid}.{ext}' final name. When false, any file whose name cannot be kept as-is is rejected with a 400 error. |
| body        | body  | any     | true     | none                                                                                                                                                                                                                                                                                                                                                                              |

<h3 id="upload-responses">Responses</h3>

| Status | Meaning                                                          | Description       | Schema |
| ------ | ---------------------------------------------------------------- | ----------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Upload successful | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input     | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-pages">pages</h1>

## redirect_to_photo

<a id="opIdredirect_to_photo"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /

```

```http
GET / HTTP/1.1

```

```javascript
fetch("/", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /`

<h3 id="redirect_to_photo-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## albums

<a id="opIdalbums"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /albums

```

```http
GET /albums HTTP/1.1

```

```javascript
fetch("/albums", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/albums',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/albums')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/albums', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/albums");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/albums", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /albums`

<h3 id="albums-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## albums_view

<a id="opIdalbums_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /albums/view/{path}

```

```http
GET /albums/view/{path} HTTP/1.1

```

```javascript
fetch("/albums/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/albums/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/albums/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/albums/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/albums/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/albums/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /albums/view/{path}`

<h3 id="albums_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## archived

<a id="opIdarchived"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /archived

```

```http
GET /archived HTTP/1.1

```

```javascript
fetch("/archived", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/archived',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/archived')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/archived', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/archived");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/archived", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /archived`

<h3 id="archived-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## archived_view

<a id="opIdarchived_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /archived/view/{path}

```

```http
GET /archived/view/{path} HTTP/1.1

```

```javascript
fetch("/archived/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/archived/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/archived/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/archived/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/archived/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/archived/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /archived/view/{path}`

<h3 id="archived_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## config

<a id="opIdconfig"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /config

```

```http
GET /config HTTP/1.1

```

```javascript
fetch("/config", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/config',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/config')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/config', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/config");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/config", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /config`

<h3 id="config-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## favicon

<a id="opIdfavicon"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /favicon.ico

```

```http
GET /favicon.ico HTTP/1.1

```

```javascript
fetch("/favicon.ico", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/favicon.ico',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/favicon.ico')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/favicon.ico', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/favicon.ico");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/favicon.ico", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /favicon.ico`

<h3 id="favicon-responses">Responses</h3>

| Status | Meaning                                                 | Description  | Schema |
| ------ | ------------------------------------------------------- | ------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | Favicon file | None   |

<aside class="success">
This operation does not require authentication
</aside>

## favorite

<a id="opIdfavorite"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /favorite

```

```http
GET /favorite HTTP/1.1

```

```javascript
fetch("/favorite", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/favorite',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/favorite')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/favorite', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/favorite");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/favorite", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /favorite`

<h3 id="favorite-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## favorite_view

<a id="opIdfavorite_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /favorite/view/{path}

```

```http
GET /favorite/view/{path} HTTP/1.1

```

```javascript
fetch("/favorite/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/favorite/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/favorite/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/favorite/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/favorite/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/favorite/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /favorite/view/{path}`

<h3 id="favorite_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## links

<a id="opIdlinks"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /links

```

```http
GET /links HTTP/1.1

```

```javascript
fetch("/links", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/links',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/links')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/links', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/links");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/links", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /links`

<h3 id="links-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## login

<a id="opIdlogin"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /login

```

```http
GET /login HTTP/1.1

```

```javascript
fetch("/login", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/login',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/login')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/login', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/login");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/login", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /login`

<h3 id="login-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## redirect_to_login

<a id="opIdredirect_to_login"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /redirect-to-login

```

```http
GET /redirect-to-login HTTP/1.1

```

```javascript
fetch("/redirect-to-login", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/redirect-to-login',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/redirect-to-login')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/redirect-to-login', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/redirect-to-login");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/redirect-to-login", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /redirect-to-login`

<h3 id="redirect_to_login-responses">Responses</h3>

| Status | Meaning                                                    | Description        | Schema |
| ------ | ---------------------------------------------------------- | ------------------ | ------ |
| 302    | [Found](https://tools.ietf.org/html/rfc7231#section-6.4.3) | Redirect to /login | None   |

<aside class="success">
This operation does not require authentication
</aside>

## sregister_sw

<a id="opIdsregister_sw"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /registerSW.js

```

```http
GET /registerSW.js HTTP/1.1

```

```javascript
fetch("/registerSW.js", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/registerSW.js',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/registerSW.js')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/registerSW.js', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/registerSW.js");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/registerSW.js", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /registerSW.js`

<h3 id="sregister_sw-responses">Responses</h3>

| Status | Meaning                                                 | Description                        | Schema |
| ------ | ------------------------------------------------------- | ---------------------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | Service worker registration script | None   |

<aside class="success">
This operation does not require authentication
</aside>

## service_worker

<a id="opIdservice_worker"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /serviceWorker.js

```

```http
GET /serviceWorker.js HTTP/1.1

```

```javascript
fetch("/serviceWorker.js", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/serviceWorker.js',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/serviceWorker.js')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/serviceWorker.js', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/serviceWorker.js");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/serviceWorker.js", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /serviceWorker.js`

<h3 id="service_worker-responses">Responses</h3>

| Status | Meaning                                                 | Description           | Schema |
| ------ | ------------------------------------------------------- | --------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | Service worker script | None   |

<aside class="success">
This operation does not require authentication
</aside>

## setting

<a id="opIdsetting"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /setting

```

```http
GET /setting HTTP/1.1

```

```javascript
fetch("/setting", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/setting',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/setting')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/setting', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/setting");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/setting", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /setting`

<h3 id="setting-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## share

<a id="opIdshare"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /share/{path}

```

```http
GET /share/{path} HTTP/1.1

```

```javascript
fetch("/share/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/share/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/share/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/share/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/share/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/share/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /share/{path}`

<h3 id="share-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## tags

<a id="opIdtags"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /tags

```

```http
GET /tags HTTP/1.1

```

```javascript
fetch("/tags", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/tags',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/tags')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/tags', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/tags");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/tags", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /tags`

<h3 id="tags-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## timeline

<a id="opIdtimeline"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /timeline

```

```http
GET /timeline HTTP/1.1

```

```javascript
fetch("/timeline", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/timeline',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/timeline')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/timeline', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/timeline");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/timeline", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /timeline`

<h3 id="timeline-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## timeline_view

<a id="opIdtimeline_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /timeline/view/{path}

```

```http
GET /timeline/view/{path} HTTP/1.1

```

```javascript
fetch("/timeline/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/timeline/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/timeline/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/timeline/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/timeline/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/timeline/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /timeline/view/{path}`

<h3 id="timeline_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## trashed

<a id="opIdtrashed"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /trashed

```

```http
GET /trashed HTTP/1.1

```

```javascript
fetch("/trashed", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/trashed',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/trashed')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/trashed', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/trashed");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/trashed", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /trashed`

<h3 id="trashed-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## trashed_view

<a id="opIdtrashed_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /trashed/view/{path}

```

```http
GET /trashed/view/{path} HTTP/1.1

```

```javascript
fetch("/trashed/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/trashed/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/trashed/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/trashed/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/trashed/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/trashed/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /trashed/view/{path}`

<h3 id="trashed_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## unauthorized

<a id="opIdunauthorized"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /unauthorized

```

```http
GET /unauthorized HTTP/1.1

```

```javascript
fetch("/unauthorized", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/unauthorized',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/unauthorized')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/unauthorized', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/unauthorized");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/unauthorized", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /unauthorized`

<h3 id="unauthorized-responses">Responses</h3>

| Status | Meaning                                                         | Description         | Schema |
| ------ | --------------------------------------------------------------- | ------------------- | ------ |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Unauthorized status | None   |

<aside class="success">
This operation does not require authentication
</aside>

## videos

<a id="opIdvideos"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /videos

```

```http
GET /videos HTTP/1.1

```

```javascript
fetch("/videos", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/videos',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/videos')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/videos', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/videos");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/videos", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /videos`

<h3 id="videos-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## videos_view

<a id="opIdvideos_view"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /videos/view/{path}

```

```http
GET /videos/view/{path} HTTP/1.1

```

```javascript
fetch("/videos/view/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/videos/view/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/videos/view/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/videos/view/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/videos/view/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/videos/view/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /videos/view/{path}`

<h3 id="videos_view-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## album_page

<a id="opIdalbum_page"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /{dynamic_album_id}

```

```http
GET /{dynamic_album_id} HTTP/1.1

```

```javascript
fetch("/{dynamic_album_id}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/{dynamic_album_id}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/{dynamic_album_id}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/{dynamic_album_id}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/{dynamic_album_id}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/{dynamic_album_id}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /{dynamic_album_id}`

<h3 id="album_page-responses">Responses</h3>

| Status | Meaning                                                        | Description     | Schema |
| ------ | -------------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)        | SPA page (HTML) | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4) | Not found       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Catch-all SPA fallback — serves index.html for valid Vue Router routes.

Paths matching `/album/<hash>` validate the album exists before serving
the SPA; invalid album hashes return 404. Rank 11 ensures specific
routes (assets at rank 10, API, pages) take priority.

<a id="opIdspa_fallback"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /{path}

```

```http
GET /{path} HTTP/1.1

```

```javascript
fetch("/{path}", {
  method: "GET",
})
  .then(function (res) {
    return res.json();
  })
  .then(function (body) {
    console.log(body);
  });
```

```ruby
require 'rest-client'
require 'json'

result = RestClient.get '/{path}',
  params: {
  }

p JSON.parse(result)

```

```python
import requests

r = requests.get('/{path}')

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/{path}', array(
        'headers' => $headers,
        'json' => $request_body,
       )
    );
    print_r($response->getBody()->getContents());
 }
 catch (\GuzzleHttp\Exception\BadResponseException $e) {
    // handle exception or api errors.
    print_r($e->getMessage());
 }

 // ...

```

```java
URL obj = new URL("/{path}");
HttpURLConnection con = (HttpURLConnection) obj.openConnection();
con.setRequestMethod("GET");
int responseCode = con.getResponseCode();
BufferedReader in = new BufferedReader(
    new InputStreamReader(con.getInputStream()));
String inputLine;
StringBuffer response = new StringBuffer();
while ((inputLine = in.readLine()) != null) {
    response.append(inputLine);
}
in.close();
System.out.println(response.toString());

```

```go
package main

import (
       "bytes"
       "net/http"
)

func main() {

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("GET", "/{path}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /{path}`

<h3 id="catch-all-spa-fallback-—-serves-index.html-for-valid-vue-router-routes.
paths-matching-`/album/<hash>`-validate-the-album-exists-before-serving
the-spa;-invalid-album-hashes-return-404.-rank-11-ensures-specific
routes-(assets-at-rank-10,-api,-pages)-take-priority.-responses">Responses</h3>

| Status | Meaning                                                 | Description                                            | Schema |
| ------ | ------------------------------------------------------- | ------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA fallback — serves index.html for Vue Router routes | None   |

<aside class="success">
This operation does not require authentication
</aside>

# Schemas

<h2 id="tocS_AlbumIndexState">AlbumIndexState</h2>
<!-- backwards compatibility -->
<a id="schemaalbumindexstate"></a>
<a id="schema_AlbumIndexState"></a>
<a id="tocSalbumindexstate"></a>
<a id="tocsalbumindexstate"></a>

```json
"idle"
```

### Properties

| Name        | Type   | Required | Restrictions | Description |
| ----------- | ------ | -------- | ------------ | ----------- |
| _anonymous_ | string | false    | none         | none        |

#### Enumerated Values

| Property    | Value     |
| ----------- | --------- |
| _anonymous_ | idle      |
| _anonymous_ | running   |
| _anonymous_ | completed |
| _anonymous_ | canceled  |
| _anonymous_ | failed    |

<h2 id="tocS_AlbumIndexStatus">AlbumIndexStatus</h2>
<!-- backwards compatibility -->
<a id="schemaalbumindexstatus"></a>
<a id="schema_AlbumIndexStatus"></a>
<a id="tocSalbumindexstatus"></a>
<a id="tocsalbumindexstatus"></a>

```json
{
  "cancelRequested": true,
  "failed": 0,
  "finishedAt": 0,
  "matched": 0,
  "processed": 0,
  "root": "string",
  "scanned": 0,
  "startedAt": 0,
  "state": "idle"
}
```

### Properties

| Name            | Type                                      | Required | Restrictions | Description |
| --------------- | ----------------------------------------- | -------- | ------------ | ----------- |
| cancelRequested | boolean                                   | true     | none         | none        |
| failed          | integer(int64)                            | true     | none         | none        |
| finishedAt      | integer,null(int64)                       | false    | none         | none        |
| matched         | integer(int64)                            | true     | none         | none        |
| processed       | integer(int64)                            | true     | none         | none        |
| root            | string,null                               | false    | none         | none        |
| scanned         | integer(int64)                            | true     | none         | none        |
| startedAt       | integer,null(int64)                       | false    | none         | none        |
| state           | [AlbumIndexState](#schemaalbumindexstate) | true     | none         | none        |

<h2 id="tocS_AlbumInfo">AlbumInfo</h2>
<!-- backwards compatibility -->
<a id="schemaalbuminfo"></a>
<a id="schema_AlbumInfo"></a>
<a id="tocSalbuminfo"></a>
<a id="tocsalbuminfo"></a>

```json
{
  "albumId": "string",
  "albumName": "string",
  "dirPath": "string",
  "parentAlbumId": "string",
  "shareList": {
    "property1": {
      "description": "string",
      "exp": 0,
      "password": "string",
      "showDownload": true,
      "showMetadata": true,
      "showUpload": true,
      "url": "string"
    },
    "property2": {
      "description": "string",
      "exp": 0,
      "password": "string",
      "showDownload": true,
      "showMetadata": true,
      "showUpload": true,
      "url": "string"
    }
  }
}
```

### Properties

| Name                       | Type                  | Required | Restrictions | Description                                                                                                       |
| -------------------------- | --------------------- | -------- | ------------ | ----------------------------------------------------------------------------------------------------------------- |
| albumId                    | string                | true     | none         | none                                                                                                              |
| albumName                  | string,null           | false    | none         | none                                                                                                              |
| dirPath                    | string,null           | false    | none         | none                                                                                                              |
| parentAlbumId              | string,null           | false    | none         | Album ID of the direct parent directory album, or `None` for top-level<br>dir albums and all user-created albums. |
| shareList                  | object                | true     | none         | none                                                                                                              |
| » **additionalProperties** | [Share](#schemashare) | false    | none         | none                                                                                                              |

<h2 id="tocS_AppConfig">AppConfig</h2>
<!-- backwards compatibility -->
<a id="schemaappconfig"></a>
<a id="schema_AppConfig"></a>
<a id="tocSappconfig"></a>
<a id="tocsappconfig"></a>

```json
{
  "address": "string",
  "authKey": "string",
  "dataHome": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "imagePath": "string",
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "password": "string",
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

### Properties

| Name                     | Type           | Required | Restrictions | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------ | -------------- | -------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| address                  | string         | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| authKey                  | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| dataHome                 | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| disableImg               | boolean        | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| fsNotifyWatcher          | boolean        | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| imagePath                | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| maxUploadSize            | string         | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| normalizeUploadFilenames | boolean        | false    | none         | NFC-normalize uploaded filenames so macOS NFD names collapse onto<br>their composed form. Optional (unlike the always-on sanitization tiers).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| password                 | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| port                     | integer(int32) | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| readOnlyMode             | boolean        | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| uploadFolder             | string         | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| useClientTimestampInfo   | boolean        | false    | none         | Trust the `lastModified` the upload client sends with each file. When<br>enabled, the value is used as the stored file modification time but<br>clamped to `[1970-01-01, now + 24h]` so a broken client clock cannot<br>push an undated file (e.g. a screenshot) into year 1970 or the distant<br>future. Disabled by default: the server uses `now()` instead of the<br>provided value, since a client's clock or timezone cannot be relied on.<br>Only affects files without embedded metadata: photos with a<br>`DateTimeOriginal` EXIF tag keep their EXIF-derived date regardless.<br>See `backend/src/router/post/post_upload.rs`<br>(`resolve_upload_timestamp`). |
| validateUploadContent    | boolean        | false    | none         | Cross-check uploads against the declared `Content-Type`. When enabled,<br>the first 512 bytes of each uploaded file are sniffed with the<br>[`infer`](https://crates.io/crates/infer) magic-byte database and the<br>detected signature must fall in the family of the extension derived<br>from the `Content-Type`: `jpg                                                                                                                                                                                                                                                                                                                                                | jpeg | jfif | jpe`→ JPEG,`tif | tiff`→<br>TIFF,`mp4 | mov | m4v`→ ISO BMFF,`mkv | webm`→ EBML,`mpeg`→ MPEG-PS,<br>and`png`, `webp`, `bmp`, `gif`, `avi`, `flv`, `wmv`1:1. Mismatches<br>and unrecognizable bytes are rejected with`400 InvalidInput`; the<br>check is signature-based only, never a full decode, so unusual-but-valid<br>variants still pass. The stored file extension remains the one derived<br>from the declared `Content-Type`. See<br>`backend/src/router/post/post_upload.rs` (`validate_upload_content`).<br>Disable only if legitimate media is being rejected. |

<h2 id="tocS_AssignAlbumData">AssignAlbumData</h2>
<!-- backwards compatibility -->
<a id="schemaassignalbumdata"></a>
<a id="schema_AssignAlbumData"></a>
<a id="tocSassignalbumdata"></a>
<a id="tocsassignalbumdata"></a>

```json
{
  "albumId": "string",
  "hash": "string",
  "onConflict": "skip"
}
```

### Properties

| Name       | Type                            | Required | Restrictions | Description |
| ---------- | ------------------------------- | -------- | ------------ | ----------- |
| albumId    | string                          | true     | none         | none        |
| hash       | string                          | true     | none         | none        |
| onConflict | [OnConflict](#schemaonconflict) | false    | none         | none        |

<h2 id="tocS_ConfigResponse">ConfigResponse</h2>
<!-- backwards compatibility -->
<a id="schemaconfigresponse"></a>
<a id="schema_ConfigResponse"></a>
<a id="tocSconfigresponse"></a>
<a id="tocsconfigresponse"></a>

```json
{
  "address": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "hasAuthKey": true,
  "hasPassword": true,
  "imagePath": "string",
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

### Properties

| Name                     | Type           | Required | Restrictions | Description |
| ------------------------ | -------------- | -------- | ------------ | ----------- |
| address                  | string         | true     | none         | none        |
| disableImg               | boolean        | true     | none         | none        |
| fsNotifyWatcher          | boolean        | true     | none         | none        |
| hasAuthKey               | boolean        | true     | none         | none        |
| hasPassword              | boolean        | true     | none         | none        |
| imagePath                | string,null    | false    | none         | none        |
| maxUploadSize            | string         | true     | none         | none        |
| normalizeUploadFilenames | boolean        | true     | none         | none        |
| port                     | integer(int32) | true     | none         | none        |
| readOnlyMode             | boolean        | true     | none         | none        |
| uploadFolder             | string         | true     | none         | none        |
| useClientTimestampInfo   | boolean        | true     | none         | none        |
| validateUploadContent    | boolean        | true     | none         | none        |

<h2 id="tocS_CreateDirAlbumData">CreateDirAlbumData</h2>
<!-- backwards compatibility -->
<a id="schemacreatediralbumdata"></a>
<a id="schema_CreateDirAlbumData"></a>
<a id="tocScreatediralbumdata"></a>
<a id="tocscreatediralbumdata"></a>

```json
{
  "name": "string",
  "parentAlbumId": "string"
}
```

### Properties

| Name          | Type   | Required | Restrictions | Description |
| ------------- | ------ | -------- | ------------ | ----------- |
| name          | string | true     | none         | none        |
| parentAlbumId | string | true     | none         | none        |

<h2 id="tocS_CreateShare">CreateShare</h2>
<!-- backwards compatibility -->
<a id="schemacreateshare"></a>
<a id="schema_CreateShare"></a>
<a id="tocScreateshare"></a>
<a id="tocscreateshare"></a>

```json
{
  "albumId": "string",
  "description": "string",
  "exp": 0,
  "password": "string",
  "showDownload": true,
  "showMetadata": true,
  "showUpload": true
}
```

### Properties

| Name         | Type           | Required | Restrictions | Description |
| ------------ | -------------- | -------- | ------------ | ----------- |
| albumId      | string         | true     | none         | none        |
| description  | string         | true     | none         | none        |
| exp          | integer(int64) | true     | none         | none        |
| password     | string,null    | false    | none         | none        |
| showDownload | boolean        | true     | none         | none        |
| showMetadata | boolean        | true     | none         | none        |
| showUpload   | boolean        | true     | none         | none        |

<h2 id="tocS_DataBaseTimestampReturn">DataBaseTimestampReturn</h2>
<!-- backwards compatibility -->
<a id="schemadatabasetimestampreturn"></a>
<a id="schema_DataBaseTimestampReturn"></a>
<a id="tocSdatabasetimestampreturn"></a>
<a id="tocsdatabasetimestampreturn"></a>

```json
{
  "abstractData": {},
  "timestamp": 0,
  "token": "string"
}
```

### Properties

| Name         | Type           | Required | Restrictions | Description |
| ------------ | -------------- | -------- | ------------ | ----------- |
| abstractData | object         | true     | none         | none        |
| timestamp    | integer(int64) | true     | none         | none        |
| token        | string         | true     | none         | none        |

<h2 id="tocS_DeleteList">DeleteList</h2>
<!-- backwards compatibility -->
<a id="schemadeletelist"></a>
<a id="schema_DeleteList"></a>
<a id="tocSdeletelist"></a>
<a id="tocsdeletelist"></a>

```json
{
  "deleteList": [0],
  "timestamp": 0
}
```

### Properties

| Name       | Type           | Required | Restrictions | Description |
| ---------- | -------------- | -------- | ------------ | ----------- |
| deleteList | [integer]      | true     | none         | none        |
| timestamp  | integer(int64) | true     | none         | none        |

<h2 id="tocS_DeleteShare">DeleteShare</h2>
<!-- backwards compatibility -->
<a id="schemadeleteshare"></a>
<a id="schema_DeleteShare"></a>
<a id="tocSdeleteshare"></a>
<a id="tocsdeleteshare"></a>

```json
{
  "albumId": "string",
  "shareId": "string"
}
```

### Properties

| Name    | Type   | Required | Restrictions | Description |
| ------- | ------ | -------- | ------------ | ----------- |
| albumId | string | true     | none         | none        |
| shareId | string | true     | none         | none        |

<h2 id="tocS_DisplayElement">DisplayElement</h2>
<!-- backwards compatibility -->
<a id="schemadisplayelement"></a>
<a id="schema_DisplayElement"></a>
<a id="tocSdisplayelement"></a>
<a id="tocsdisplayelement"></a>

```json
{
  "displayHeight": 0,
  "displayWidth": 0
}
```

### Properties

| Name          | Type           | Required | Restrictions | Description |
| ------------- | -------------- | -------- | ------------ | ----------- |
| displayHeight | integer(int32) | true     | none         | none        |
| displayWidth  | integer(int32) | true     | none         | none        |

<h2 id="tocS_EditFlagsData">EditFlagsData</h2>
<!-- backwards compatibility -->
<a id="schemaeditflagsdata"></a>
<a id="schema_EditFlagsData"></a>
<a id="tocSeditflagsdata"></a>
<a id="tocseditflagsdata"></a>

```json
{
  "indexArray": [0],
  "isArchived": true,
  "isFavorite": true,
  "isTrashed": true,
  "timestamp": 0
}
```

### Properties

| Name       | Type           | Required | Restrictions | Description |
| ---------- | -------------- | -------- | ------------ | ----------- |
| indexArray | [integer]      | true     | none         | none        |
| isArchived | boolean,null   | false    | none         | none        |
| isFavorite | boolean,null   | false    | none         | none        |
| isTrashed  | boolean,null   | false    | none         | none        |
| timestamp  | integer(int64) | true     | none         | none        |

<h2 id="tocS_EditRatingData">EditRatingData</h2>
<!-- backwards compatibility -->
<a id="schemaeditratingdata"></a>
<a id="schema_EditRatingData"></a>
<a id="tocSeditratingdata"></a>
<a id="tocseditratingdata"></a>

```json
{
  "indexArray": [0],
  "rating": 0,
  "timestamp": 0
}
```

### Properties

| Name       | Type                | Required | Restrictions | Description                        |
| ---------- | ------------------- | -------- | ------------ | ---------------------------------- |
| indexArray | [integer]           | true     | none         | none                               |
| rating     | integer,null(int32) | false    | none         | Rating value 0–5, or null to clear |
| timestamp  | integer(int64)      | true     | none         | none                               |

<h2 id="tocS_EditShare">EditShare</h2>
<!-- backwards compatibility -->
<a id="schemaeditshare"></a>
<a id="schema_EditShare"></a>
<a id="tocSeditshare"></a>
<a id="tocseditshare"></a>

```json
{
  "albumId": "string",
  "share": {
    "description": "string",
    "exp": 0,
    "password": "string",
    "showDownload": true,
    "showMetadata": true,
    "showUpload": true,
    "url": "string"
  }
}
```

### Properties

| Name    | Type                  | Required | Restrictions | Description |
| ------- | --------------------- | -------- | ------------ | ----------- |
| albumId | string                | true     | none         | none        |
| share   | [Share](#schemashare) | true     | none         | none        |

<h2 id="tocS_EditTagsData">EditTagsData</h2>
<!-- backwards compatibility -->
<a id="schemaedittagsdata"></a>
<a id="schema_EditTagsData"></a>
<a id="tocSedittagsdata"></a>
<a id="tocsedittagsdata"></a>

```json
{
  "addTagsArray": ["string"],
  "indexArray": [0],
  "removeTagsArray": ["string"],
  "timestamp": 0
}
```

### Properties

| Name            | Type           | Required | Restrictions | Description |
| --------------- | -------------- | -------- | ------------ | ----------- |
| addTagsArray    | [string]       | true     | none         | none        |
| indexArray      | [integer]      | true     | none         | none        |
| removeTagsArray | [string]       | true     | none         | none        |
| timestamp       | integer(int64) | true     | none         | none        |

<h2 id="tocS_FsCompletion">FsCompletion</h2>
<!-- backwards compatibility -->
<a id="schemafscompletion"></a>
<a id="schema_FsCompletion"></a>
<a id="tocSfscompletion"></a>
<a id="tocsfscompletion"></a>

```json
{
  "children": ["string"],
  "is_default": true,
  "roots": ["string"]
}
```

### Properties

| Name       | Type     | Required | Restrictions | Description |
| ---------- | -------- | -------- | ------------ | ----------- |
| children   | [string] | true     | none         | none        |
| is_default | boolean  | true     | none         | none        |
| roots      | [string] | true     | none         | none        |

<h2 id="tocS_IndexAlbumRequest">IndexAlbumRequest</h2>
<!-- backwards compatibility -->
<a id="schemaindexalbumrequest"></a>
<a id="schema_IndexAlbumRequest"></a>
<a id="tocSindexalbumrequest"></a>
<a id="tocsindexalbumrequest"></a>

```json
{
  "album": "string"
}
```

### Properties

| Name  | Type   | Required | Restrictions | Description |
| ----- | ------ | -------- | ------------ | ----------- |
| album | string | true     | none         | none        |

<h2 id="tocS_IndexImageRequest">IndexImageRequest</h2>
<!-- backwards compatibility -->
<a id="schemaindeximagerequest"></a>
<a id="schema_IndexImageRequest"></a>
<a id="tocSindeximagerequest"></a>
<a id="tocsindeximagerequest"></a>

```json
{
  "album": "string",
  "image": "string"
}
```

### Properties

| Name  | Type        | Required | Restrictions | Description |
| ----- | ----------- | -------- | ------------ | ----------- |
| album | string,null | false    | none         | none        |
| image | string      | true     | none         | none        |

<h2 id="tocS_OnConflict">OnConflict</h2>
<!-- backwards compatibility -->
<a id="schemaonconflict"></a>
<a id="schema_OnConflict"></a>
<a id="tocSonconflict"></a>
<a id="tocsonconflict"></a>

```json
"skip"
```

### Properties

| Name        | Type   | Required | Restrictions | Description |
| ----------- | ------ | -------- | ------------ | ----------- |
| _anonymous_ | string | false    | none         | none        |

#### Enumerated Values

| Property    | Value   |
| ----------- | ------- |
| _anonymous_ | skip    |
| _anonymous_ | rename  |
| _anonymous_ | replace |

<h2 id="tocS_PartialUpdateConfigRequest">PartialUpdateConfigRequest</h2>
<!-- backwards compatibility -->
<a id="schemapartialupdateconfigrequest"></a>
<a id="schema_PartialUpdateConfigRequest"></a>
<a id="tocSpartialupdateconfigrequest"></a>
<a id="tocspartialupdateconfigrequest"></a>

```json
{
  "address": "string",
  "authKey": "string",
  "disableImg": true,
  "fsNotifyWatcher": true,
  "maxUploadSize": "string",
  "normalizeUploadFilenames": true,
  "port": 0,
  "readOnlyMode": true,
  "uploadFolder": "string",
  "useClientTimestampInfo": true,
  "validateUploadContent": true
}
```

### Properties

| Name                     | Type                | Required | Restrictions | Description                                                         |
| ------------------------ | ------------------- | -------- | ------------ | ------------------------------------------------------------------- |
| address                  | string,null         | false    | none         | none                                                                |
| authKey                  | string,null         | false    | none         | none                                                                |
| disableImg               | boolean,null        | false    | none         | none                                                                |
| fsNotifyWatcher          | boolean,null        | false    | none         | none                                                                |
| maxUploadSize            | string,null         | false    | none         | `None` = don't touch; `Some("")` resets to the default ("100MiB").  |
| normalizeUploadFilenames | boolean,null        | false    | none         | none                                                                |
| port                     | integer,null(int32) | false    | none         | none                                                                |
| readOnlyMode             | boolean,null        | false    | none         | none                                                                |
| uploadFolder             | string,null         | false    | none         | `None` = don't touch; `Some("")` resets to the default ("uploads"). |
| useClientTimestampInfo   | boolean,null        | false    | none         | none                                                                |
| validateUploadContent    | boolean,null        | false    | none         | none                                                                |

<h2 id="tocS_Prefetch">Prefetch</h2>
<!-- backwards compatibility -->
<a id="schemaprefetch"></a>
<a id="schema_Prefetch"></a>
<a id="tocSprefetch"></a>
<a id="tocsprefetch"></a>

```json
{
  "dataLength": 0,
  "locateTo": 0,
  "timestamp": 0
}
```

### Properties

| Name       | Type           | Required | Restrictions | Description |
| ---------- | -------------- | -------- | ------------ | ----------- |
| dataLength | integer        | true     | none         | none        |
| locateTo   | integer,null   | false    | none         | none        |
| timestamp  | integer(int64) | true     | none         | none        |

<h2 id="tocS_PrefetchReturn">PrefetchReturn</h2>
<!-- backwards compatibility -->
<a id="schemaprefetchreturn"></a>
<a id="schema_PrefetchReturn"></a>
<a id="tocSprefetchreturn"></a>
<a id="tocsprefetchreturn"></a>

```json
{
  "prefetch": {
    "dataLength": 0,
    "locateTo": 0,
    "timestamp": 0
  },
  "resolvedShareOpt": {},
  "token": "string"
}
```

### Properties

| Name             | Type                        | Required | Restrictions | Description |
| ---------------- | --------------------------- | -------- | ------------ | ----------- |
| prefetch         | [Prefetch](#schemaprefetch) | true     | none         | none        |
| resolvedShareOpt | any                         | false    | none         | none        |

oneOf

| Name          | Type | Required | Restrictions | Description |
| ------------- | ---- | -------- | ------------ | ----------- |
| » _anonymous_ | null | false    | none         | none        |

xor

| Name          | Type                                  | Required | Restrictions | Description |
| ------------- | ------------------------------------- | -------- | ------------ | ----------- |
| » _anonymous_ | [ResolvedShare](#schemaresolvedshare) | false    | none         | none        |

continued

| Name  | Type   | Required | Restrictions | Description |
| ----- | ------ | -------- | ------------ | ----------- |
| token | string | true     | none         | none        |

<h2 id="tocS_ResolvedShare">ResolvedShare</h2>
<!-- backwards compatibility -->
<a id="schemaresolvedshare"></a>
<a id="schema_ResolvedShare"></a>
<a id="tocSresolvedshare"></a>
<a id="tocsresolvedshare"></a>

```json
{
  "albumId": "string",
  "albumTitle": "string",
  "share": {
    "description": "string",
    "exp": 0,
    "password": "string",
    "showDownload": true,
    "showMetadata": true,
    "showUpload": true,
    "url": "string"
  }
}
```

### Properties

| Name       | Type                  | Required | Restrictions | Description |
| ---------- | --------------------- | -------- | ------------ | ----------- |
| albumId    | string                | true     | none         | none        |
| albumTitle | string,null           | false    | none         | none        |
| share      | [Share](#schemashare) | true     | none         | none        |

<h2 id="tocS_RotateImageRequest">RotateImageRequest</h2>
<!-- backwards compatibility -->
<a id="schemarotateimagerequest"></a>
<a id="schema_RotateImageRequest"></a>
<a id="tocSrotateimagerequest"></a>
<a id="tocsrotateimagerequest"></a>

```json
{
  "hash": "string"
}
```

### Properties

| Name | Type   | Required | Restrictions | Description                 |
| ---- | ------ | -------- | ------------ | --------------------------- |
| hash | string | true     | none         | Hash of the image to rotate |

<h2 id="tocS_Row">Row</h2>
<!-- backwards compatibility -->
<a id="schemarow"></a>
<a id="schema_Row"></a>
<a id="tocSrow"></a>
<a id="tocsrow"></a>

```json
{
  "displayElements": [
    {
      "displayHeight": 0,
      "displayWidth": 0
    }
  ],
  "end": 0,
  "rowIndex": 0,
  "start": 0
}
```

### Properties

| Name            | Type                                      | Required | Restrictions | Description |
| --------------- | ----------------------------------------- | -------- | ------------ | ----------- |
| displayElements | [[DisplayElement](#schemadisplayelement)] | true     | none         | none        |
| end             | integer                                   | true     | none         | none        |
| rowIndex        | integer                                   | true     | none         | none        |
| start           | integer                                   | true     | none         | none        |

<h2 id="tocS_ScrollBarData">ScrollBarData</h2>
<!-- backwards compatibility -->
<a id="schemascrollbardata"></a>
<a id="schema_ScrollBarData"></a>
<a id="tocSscrollbardata"></a>
<a id="tocsscrollbardata"></a>

```json
{
  "index": 0,
  "month": 0,
  "year": 0
}
```

### Properties

| Name  | Type    | Required | Restrictions | Description |
| ----- | ------- | -------- | ------------ | ----------- |
| index | integer | true     | none         | none        |
| month | integer | true     | none         | none        |
| year  | integer | true     | none         | none        |

<h2 id="tocS_SetAlbumCover">SetAlbumCover</h2>
<!-- backwards compatibility -->
<a id="schemasetalbumcover"></a>
<a id="schema_SetAlbumCover"></a>
<a id="tocSsetalbumcover"></a>
<a id="tocssetalbumcover"></a>

```json
{
  "albumId": "string",
  "coverHash": "string"
}
```

Payload for updating a specific album's cover image.

### Properties

| Name      | Type   | Required | Restrictions | Description                            |
| --------- | ------ | -------- | ------------ | -------------------------------------- |
| albumId   | string | true     | none         | none                                   |
| coverHash | string | true     | none         | The hash of the image to set as cover. |

<h2 id="tocS_SetAlbumTitle">SetAlbumTitle</h2>
<!-- backwards compatibility -->
<a id="schemasetalbumtitle"></a>
<a id="schema_SetAlbumTitle"></a>
<a id="tocSsetalbumtitle"></a>
<a id="tocssetalbumtitle"></a>

```json
{
  "albumId": "string",
  "title": "string"
}
```

Payload for renaming an album.

### Properties

| Name    | Type        | Required | Restrictions | Description |
| ------- | ----------- | -------- | ------------ | ----------- |
| albumId | string      | true     | none         | none        |
| title   | string,null | false    | none         | none        |

<h2 id="tocS_SetUserDefinedDescription">SetUserDefinedDescription</h2>
<!-- backwards compatibility -->
<a id="schemasetuserdefineddescription"></a>
<a id="schema_SetUserDefinedDescription"></a>
<a id="tocSsetuserdefineddescription"></a>
<a id="tocssetuserdefineddescription"></a>

```json
{
  "description": "string",
  "index": 0,
  "timestamp": 0
}
```

### Properties

| Name        | Type           | Required | Restrictions | Description |
| ----------- | -------------- | -------- | ------------ | ----------- |
| description | string,null    | false    | none         | none        |
| index       | integer        | true     | none         | none        |
| timestamp   | integer(int64) | true     | none         | none        |

<h2 id="tocS_Share">Share</h2>
<!-- backwards compatibility -->
<a id="schemashare"></a>
<a id="schema_Share"></a>
<a id="tocSshare"></a>
<a id="tocsshare"></a>

```json
{
  "description": "string",
  "exp": 0,
  "password": "string",
  "showDownload": true,
  "showMetadata": true,
  "showUpload": true,
  "url": "string"
}
```

### Properties

| Name         | Type           | Required | Restrictions | Description |
| ------------ | -------------- | -------- | ------------ | ----------- |
| description  | string         | true     | none         | none        |
| exp          | integer(int64) | true     | none         | none        |
| password     | string,null    | false    | none         | none        |
| showDownload | boolean        | true     | none         | none        |
| showMetadata | boolean        | true     | none         | none        |
| showUpload   | boolean        | true     | none         | none        |
| url          | string         | true     | none         | none        |

<h2 id="tocS_TagInfo">TagInfo</h2>
<!-- backwards compatibility -->
<a id="schemataginfo"></a>
<a id="schema_TagInfo"></a>
<a id="tocStaginfo"></a>
<a id="tocstaginfo"></a>

```json
{
  "number": 0,
  "tag": "string"
}
```

### Properties

| Name   | Type    | Required | Restrictions | Description |
| ------ | ------- | -------- | ------------ | ----------- |
| number | integer | true     | none         | none        |
| tag    | string  | true     | none         | none        |

<h2 id="tocS_UpdatePasswordRequest">UpdatePasswordRequest</h2>
<!-- backwards compatibility -->
<a id="schemaupdatepasswordrequest"></a>
<a id="schema_UpdatePasswordRequest"></a>
<a id="tocSupdatepasswordrequest"></a>
<a id="tocsupdatepasswordrequest"></a>

```json
{
  "oldPassword": "string",
  "password": "string"
}
```

### Properties

| Name        | Type        | Required | Restrictions | Description |
| ----------- | ----------- | -------- | ------------ | ----------- |
| oldPassword | string,null | false    | none         | none        |
| password    | string,null | false    | none         | none        |
