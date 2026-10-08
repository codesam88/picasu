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

Self-hosted photo gallery API. This document is generated from the `#[utoipa::path]` annotations in `backend/src/router`; see `docs/openapi-generator.md`.

Web: <a href="https://github.com/codesam88/picasu">picasu</a>
License: MIT

<h1 id="picasu-auth">auth</h1>

Authentication and token renewal

## Sign in with the account password and return a 14-day admin JWT.

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

The posted password is trimmed and compared against the configured one; a
match returns the signed admin token as a JSON string. Tokens are signed
with `authKey`, or with a secret generated once per process when `authKey`
is unset, so changing `authKey` invalidates every token issued under the
previous one.

Corner cases: While no password is configured, sign-in succeeds without
one: every input, including an empty string, is accepted.

Errors: 401 password does not match the configured one.

> Body parameter

```
string

```

<h3 id="sign-in-with-the-account-password-and-return-a-14-day-admin-jwt.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description |
| ---- | ---- | ------ | -------- | ----------- |
| body | body | string | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="sign-in-with-the-account-password-and-return-a-14-day-admin-jwt.-responses">Responses</h3>

| Status | Meaning                                                         | Description                                                                                | Schema |
| ------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)         | JWT token                                                                                  | string |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Exchange an expired image-serving token for a freshly issued one.

<a id="opIdrenew_hash_token"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/renew-hash-token \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json'

```

```http
POST /post/renew-hash-token HTTP/1.1

Content-Type: application/json
Accept: application/json

```

```javascript
const inputBody = '{
  "expiredHashToken": "string"
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'application/json'
};

fetch('/post/renew-hash-token',
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
  'Accept' => 'application/json'
}

result = RestClient.post '/post/renew-hash-token',
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

r = requests.post('/post/renew-hash-token', headers = headers)

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
    $response = $client->request('POST','/post/renew-hash-token', array(
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
URL obj = new URL("/post/renew-hash-token");
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
    req, err := http.NewRequest("POST", "/post/renew-hash-token", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/renew-hash-token`

The submitted token is decoded with expiration checking disabled, so an
expired but correctly signed token is accepted. The returned token keeps
the original `hash`, `assetId` and `allowOriginal` claims and expires 300
seconds after it is issued.

Corner cases: The request must also carry a valid, unexpired prefetch
timestamp bearer token, and the `timestamp` claim of the submitted token
must match it — renewing only extends the lifetime of the same snapshot.

Errors: 400 unusable request body — 401 unverifiable signature, mismatched
`timestamp`, or a missing or invalid timestamp bearer token — 500 internal
failure.

> Body parameter

```json
{
  "expiredHashToken": "string"
}
```

<h3 id="exchange-an-expired-image-serving-token-for-a-freshly-issued-one.-parameters">Parameters</h3>

| Name | In   | Type                                    | Required | Description |
| ---- | ---- | --------------------------------------- | -------- | ----------- |
| body | body | [RenewHashToken](#schemarenewhashtoken) | true     | none        |

> Example responses

> 200 Response

```json
{
  "token": "string"
}
```

<h3 id="exchange-an-expired-image-serving-token-for-a-freshly-issued-one.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                                              |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Hash token renewed                                                                         | [RenewHashTokenReturn](#schemarenewhashtokenreturn) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None                                                |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                                |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                                |

<aside class="success">
This operation does not require authentication
</aside>

## Exchange an expired prefetch timestamp token for a freshly issued one.

<a id="opIdrenew_timestamp_token"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/renew-timestamp-token \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json'

```

```http
POST /post/renew-timestamp-token HTTP/1.1

Content-Type: application/json
Accept: application/json

```

```javascript
const inputBody = '{
  "token": "string"
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'application/json'
};

fetch('/post/renew-timestamp-token',
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
  'Accept' => 'application/json'
}

result = RestClient.post '/post/renew-timestamp-token',
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

r = requests.post('/post/renew-timestamp-token', headers = headers)

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
    $response = $client->request('POST','/post/renew-timestamp-token', array(
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
URL obj = new URL("/post/renew-timestamp-token");
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
    req, err := http.NewRequest("POST", "/post/renew-timestamp-token", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/renew-timestamp-token`

Accepts share credentials (`x-album-id` + `x-share-id` headers or
`albumId` + `shareId` query parameters) or an admin JWT cookie. The
submitted token is decoded with expiration checking disabled and reissued
with its original snapshot `timestamp` and resolved share intact, expiring
300 seconds later.

Corner cases: Re-renewing keeps addressing the same snapshot, so only the
expiry changes and the underlying data is not re-read.

Errors: 400 only one of the share credential header or query pair given —
401 missing or invalid credentials, or an unverifiable token signature —
500 internal failure.

> Body parameter

```json
{
  "token": "string"
}
```

<h3 id="exchange-an-expired-prefetch-timestamp-token-for-a-freshly-issued-one.-parameters">Parameters</h3>

| Name | In   | Type                                              | Required | Description |
| ---- | ---- | ------------------------------------------------- | -------- | ----------- |
| body | body | [RenewTimestampToken](#schemarenewtimestamptoken) | true     | none        |

> Example responses

> 200 Response

```json
{
  "token": "string"
}
```

<h3 id="exchange-an-expired-prefetch-timestamp-token-for-a-freshly-issued-one.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                                                        |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Timestamp token renewed                                                                    | [RenewTimestampTokenReturn](#schemarenewtimestamptokenreturn) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None                                                          |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                                          |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                                          |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-albums">albums</h1>

Albums and shares: creation, assignment, covers, titles, descriptions, share links

## List every album the metadata tree knows.

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

Returns one entry per album with `albumId`, `albumName`, the album's
`shareList` share definitions, `dirPath` and `parentAlbumId`.

Corner cases: `dirPath` is reported relative to the configured `imagePath`
so clients never handle the absolute library location, falling back to the
absolute path for albums outside that root; `parentAlbumId` names the direct
parent directory album and is `null` for top-level dir albums and all
manually created albums.

Errors: 401 no valid admin credentials.

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

<h3 id="list-every-album-the-metadata-tree-knows.-responses">Responses</h3>

| Status | Meaning                                                         | Description                                                                                | Schema |
| ------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)         | List of albums                                                                             | Inline |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |

<h3 id="list-every-album-the-metadata-tree-knows.-responseschema">Response Schema</h3>

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

## Create a subdirectory under a dir-album and register it as an album.

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

`name` is trimmed and must be a single path segment — no separator, no `.`
or `..`. The directory is created directly under the existing parent
album's own directory, registered as an album, and the parent album is
refreshed before the response returns.

Corner cases: The new album's ID is returned as a string. A failure after
the directory was created but before the album was registered leaves the
empty directory behind.

Errors: 400 unusable `name`, or a `parentAlbumId` that names no dir-album —
401 missing or invalid credentials — 405 read-only mode — 500 the directory
could not be created or the album not registered.

> Body parameter

```json
{
  "name": "string",
  "parentAlbumId": "string"
}
```

<h3 id="create-a-subdirectory-under-a-dir-album-and-register-it-as-an-album.-parameters">Parameters</h3>

| Name | In   | Type                                            | Required | Description |
| ---- | ---- | ----------------------------------------------- | -------- | ----------- |
| body | body | [CreateDirAlbumData](#schemacreatediralbumdata) | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="create-a-subdirectory-under-a-dir-album-and-register-it-as-an-album.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | New album ID                                                                               | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Create a share for an album and return its generated id.

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

`albumId` must name an existing album row. The returned id is a freshly
generated 64-character lowercase alphanumeric string that becomes the
album's `shareList` key. `exp` is stored verbatim as unix seconds, where
`0` marks a share that never expires; `password`, `showMetadata`,
`showDownload` and `showUpload` are stored as posted and decide what later
requests carrying the share may do.

Corner cases: The id is generated, never supplied by the caller, so a
second share for the same album is a separate entry. Nothing is written
unless the album exists.

Errors: 400 unusable request body — 401 missing or invalid credentials —
404 `albumId` names no album — 405 read-only mode — 500 storage failure.

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

<h3 id="create-a-share-for-an-album-and-return-its-generated-id.-parameters">Parameters</h3>

| Name | In   | Type                              | Required | Description |
| ---- | ---- | --------------------------------- | -------- | ----------- |
| body | body | [CreateShare](#schemacreateshare) | true     | none        |

> Example responses

> 200 Response

```
"string"
```

<h3 id="create-a-share-for-an-album-and-return-its-generated-id.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Share link created                                                                         | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Album not found                                                                            | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Move one asset, or one album directory, into another album.

<a id="opIdassign_album"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/assign_album \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json'

```

```http
PUT /put/assign_album HTTP/1.1

Content-Type: application/json
Accept: application/json

```

```javascript
const inputBody = '{
  "albumId": "string",
  "assetId": "string",
  "onConflict": "skip"
}';
const headers = {
  'Content-Type':'application/json',
  'Accept':'application/json'
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
  'Content-Type' => 'application/json',
  'Accept' => 'application/json'
}

result = RestClient.put '/put/assign_album',
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

r = requests.put('/put/assign_album', headers = headers)

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
        "Accept": []string{"application/json"},
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

`assetId` addresses the asset record and `albumId` the destination album,
whose directory is resolved from the album directory cache. Only the file at
the asset's own path moves, never hash-matched duplicates: the stored path
and album membership are rewritten to match, an album moves as its whole
directory tree with every descendant record's path rewritten, and a `.xmp`
sidecar rides along with the file. The destination album's stats are
refreshed and the query-cache version bumped before the response returns, so
the next prefetch already reflects the move.

Corner cases: `onConflict` is required and has no default — `skip` leaves an
existing destination untouched and reports `skipped`, `rename` lands under a
suffixed name and reports `renamedFrom`, a plain move reports `moved`. An
asset already sitting in the destination directory reports `moved` without
anything being rewritten, and an album moved into itself or into one of its
own sub-albums is rejected. Compressed thumbnails and videos are keyed by
content hash, not by path, so no derivative moves or is invalidated.

Errors: 400 unknown asset or album id, a source path or album directory that
no longer exists, or a destination album that has no directory — 401 missing
or invalid admin credentials; share tokens are not accepted — 405 read-only
mode — 500 storage failure.

> Body parameter

```json
{
  "albumId": "string",
  "assetId": "string",
  "onConflict": "skip"
}
```

<h3 id="move-one-asset,-or-one-album-directory,-into-another-album.-parameters">Parameters</h3>

| Name | In   | Type                                      | Required | Description |
| ---- | ---- | ----------------------------------------- | -------- | ----------- |
| body | body | [AssignAlbumData](#schemaassignalbumdata) | true     | none        |

> Example responses

> 200 Response

```json
{
  "outcome": "moved"
}
```

<h3 id="move-one-asset,-or-one-album-directory,-into-another-album.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                              |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Item assigned to album                                                                     | [AssignResult](#schemaassignresult) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input or item not found                                                            | None                                |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None                                |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                |

<aside class="success">
This operation does not require authentication
</aside>

## Remove one share from an album.

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

`albumId` names the album and `shareId` the entry to drop from its share
list. The removal is committed to the album's stored share list, no
`.albuminfo.xmp` sidecar is written, and the in-memory tree is rebuilt
before the call returns.

Corner cases: removing a share that is not present, or addressing an
`albumId` that holds no album row, is a silent no-op that still answers 200
rather than a 404.

Errors: 400 malformed body — 401 missing or invalid admin credentials; share
tokens are not accepted — 405 read-only mode — 500 storage failure.

> Body parameter

```json
{
  "albumId": "string",
  "shareId": "string"
}
```

<h3 id="remove-one-share-from-an-album.-parameters">Parameters</h3>

| Name | In   | Type                              | Required | Description |
| ---- | ---- | --------------------------------- | -------- | ----------- |
| body | body | [DeleteShare](#schemadeleteshare) | true     | none        |

<h3 id="remove-one-share-from-an-album.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Share deleted                                                                              | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Create or replace one of an album's share entries.

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

The submitted `share` object is stored under the key `share.url`, replacing
any entry already held under that key, so a share is edited by posting a
complete object and every capability and field takes its submitted value
rather than the existing one. The payload is never checked against the
album, no `.albuminfo.xmp` sidecar is written, and the in-memory tree is
rebuilt before the call returns.

Corner cases: an `albumId` that holds no album row is a silent no-op that
still answers 200.

Errors: 400 malformed body — 401 missing or invalid admin credentials; share
tokens are not accepted — 405 read-only mode — 500 storage failure.

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

<h3 id="create-or-replace-one-of-an-album's-share-entries.-parameters">Parameters</h3>

| Name | In   | Type                          | Required | Description |
| ---- | ---- | ----------------------------- | -------- | ----------- |
| body | body | [EditShare](#schemaeditshare) | true     | none        |

<h3 id="create-or-replace-one-of-an-album's-share-entries.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Share updated                                                                              | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Set the cover image of an album.

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
  "coverAssetId": "string"
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

`albumId` must resolve to an album record and `coverAssetId` to an existing
record. The album's `cover` becomes `coverAssetId` and its `thumbhash` is
overwritten with the cover's, and the in-memory tree is rebuilt before the
call returns.

Corner cases: the cover's kind is never checked, so any asset can serve as
the cover, and no `.albuminfo.xmp` sidecar is written, so the new cover is
stored in the database only.

Errors: 400 `albumId` names a media record — 401 missing or invalid admin
credentials; share tokens are not accepted — 404 `albumId` or `coverAssetId`
names no record — 405 read-only mode — 500 storage failure.

> Body parameter

```json
{
  "albumId": "string",
  "coverAssetId": "string"
}
```

<h3 id="set-the-cover-image-of-an-album.-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [SetAlbumCover](#schemasetalbumcover) | true     | none        |

<h3 id="set-the-cover-image-of-an-album.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Album cover updated                                                                        | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                                                                  | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Set the display title of an album.

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

`albumId` names the album to edit and `title` its new display title. Both
the raw display `title` and `custom_title`, the value written to the album's
`.albuminfo.xmp` sidecar, take the submitted value, and the in-memory tree
is rebuilt before the call returns.

Corner cases: `title: null` clears `custom_title` and falls `title` back to
the directory-derived default, so a cleared title leaves no empty string
behind.

Errors: 400 malformed body — 401 missing or invalid admin or share
credentials — 405 read-only mode — 500 storage failure.

> Body parameter

```json
{
  "albumId": "string",
  "title": "string"
}
```

<h3 id="set-the-display-title-of-an-album.-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [SetAlbumTitle](#schemasetalbumtitle) | true     | none        |

<h3 id="set-the-display-title-of-an-album.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Album title updated                                                                        | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Set or clear the user-defined description of an asset.

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

`index` addresses the asset in the snapshot named by `timestamp`, and
`description` replaces the field: `null` clears it, other text is
NFC-normalized and filtered to valid XML characters. The value is written to
the asset's `.albuminfo.xmp` sidecar on a best-effort basis, persisted, and
the in-memory tree is rebuilt before the call returns. Share credentials
(`x-album-id` plus `x-share-id`, or `albumId` plus `shareId`) are accepted
alongside the admin JWT cookie.

Corner cases: the share's `showMetadata` flag is not consulted, so a share
without metadata rights may still write a description. An index whose asset
record no longer exists is skipped and still answers 200, and a failed
sidecar write is logged rather than failing the request.

Errors: 400 malformed body — 401 missing or invalid admin or share
credentials — 405 read-only mode — 500 unknown snapshot, out-of-range index,
or storage failure.

> Body parameter

```json
{
  "description": "string",
  "index": 0,
  "timestamp": 0
}
```

<h3 id="set-or-clear-the-user-defined-description-of-an-asset.-parameters">Parameters</h3>

| Name | In   | Type                                                          | Required | Description |
| ---- | ---- | ------------------------------------------------------------- | -------- | ----------- |
| body | body | [SetUserDefinedDescription](#schemasetuserdefineddescription) | true     | none        |

<h3 id="set-or-clear-the-user-defined-description-of-an-asset.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Description updated                                                                        | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-assets">assets</h1>

Per-asset metadata and editing: flags, rating, tags, rotation, thumbnails, deletion

## Delete the listed assets: files, sidecars and metadata rows are removed.

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
  "assetIds": [
    "string"
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

`assetIds` are resolved by asset ID; each entry's file and `.xmp` sidecar
are removed from disk and its metadata row is dropped. Every album the
deletion touched is refreshed before the response returns.

Corner cases: An empty `assetIds` is a 400. IDs are processed in order, so
a malformed or unresolvable ID aborts the request part-way and files
already removed by earlier entries stay deleted. Deleting an album also
removes every descendant asset, sidecar and directory below it, and a
compressed thumbnail is kept while another asset still references the same
content hash.

Errors: 400 empty `assetIds`, or an ID that exceeds 64 bytes — 401 missing
or invalid credentials — 404 an ID names no asset — 405 read-only mode —
500 storage failure.

> Body parameter

```json
{
  "assetIds": ["string"],
  "timestamp": 0
}
```

<h3 id="delete-the-listed-assets:-files,-sidecars-and-metadata-rows-are-removed.-parameters">Parameters</h3>

| Name | In   | Type                            | Required | Description |
| ---- | ---- | ------------------------------- | -------- | ----------- |
| body | body | [DeleteList](#schemadeletelist) | true     | none        |

<h3 id="delete-the-listed-assets:-files,-sidecars-and-metadata-rows-are-removed.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Data deleted                                                                               | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Asset not found                                                                            | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the full metadata of a single asset.

<a id="opIdget_metadata"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/metadata/{asset_id}?timestamp=0

```

```http
GET /get/metadata/{asset_id}?timestamp=0 HTTP/1.1

```

```javascript
fetch("/get/metadata/{asset_id}?timestamp=0", {
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

result = RestClient.get '/get/metadata/{asset_id}',
  params: {
  'timestamp' => 'integer(int64)'
}

p JSON.parse(result)

```

```python
import requests

r = requests.get('/get/metadata/{asset_id}', params={
  'timestamp': '0'
})

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$client = new \GuzzleHttp\Client();

// Define array of request body.
$request_body = array();

try {
    $response = $client->request('GET','/get/metadata/{asset_id}', array(
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
URL obj = new URL("/get/metadata/{asset_id}?timestamp=0");
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
    req, err := http.NewRequest("GET", "/get/metadata/{asset_id}", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`GET /get/metadata/{asset_id}`

List rows carry only lean identity fields, so the detail view, sidebar and
edit prefill fetch the complete asset here. The wire shape is unchanged:
identity fields come from the asset's identity record, metadata fields from
its stored payload.

Corner cases: `timestamp` is required and must equal the prefetch token's
`timestamp` claim. When the token resolves to a share with
`show_metadata: false`, the metadata fields — including the stored path —
are cleared before responding, so a share that hides metadata cannot leak it
here.

Errors: 400 invalid `asset_id` — 401 missing, invalid, or mismatched
prefetch token — 404 unknown `asset_id` — 500 the asset record could not be
composed.

<h3 id="serve-the-full-metadata-of-a-single-asset.-parameters">Parameters</h3>

| Name      | In    | Type           | Required | Description                                     |
| --------- | ----- | -------------- | -------- | ----------------------------------------------- |
| asset_id  | path  | string         | true     | Asset to read the full metadata for             |
| timestamp | query | integer(int64) | true     | Must equal the prefetch token's timestamp claim |

<h3 id="serve-the-full-metadata-of-a-single-asset.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Full metadata record for the asset                                                         | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Unknown asset_id                                                                           | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Set or clear the trash flag on the listed assets.

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

`indexArray` addresses the assets in the snapshot named by `timestamp`, and
a sent `isTrashed` value lands on each asset's own record; no file on disk
is touched. Every album that lost or regained a member is refreshed in the
background, so its counts may still be catching up when the call returns.

Corner cases: the removed `isFavorite` and `isArchived` keys are still
accepted and ignored rather than rejected, and an absent `isTrashed` leaves
every flag as it was. An index whose asset record no longer exists is
skipped and still answers 200.

Errors: 400 malformed body — 401 missing or invalid admin credentials; share
tokens are not accepted — 405 read-only mode — 500 unknown snapshot,
out-of-range index, or storage failure.

> Body parameter

```json
{
  "indexArray": [0],
  "isTrashed": true,
  "timestamp": 0
}
```

<h3 id="set-or-clear-the-trash-flag-on-the-listed-assets.-parameters">Parameters</h3>

| Name | In   | Type                                  | Required | Description |
| ---- | ---- | ------------------------------------- | -------- | ----------- |
| body | body | [EditFlagsData](#schemaeditflagsdata) | true     | none        |

<h3 id="set-or-clear-the-trash-flag-on-the-listed-assets.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Flags updated                                                                              | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Rate the listed assets 0–5, or clear the rating.

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

`indexArray` addresses the assets in the snapshot named by `timestamp`, and
`rating` carries 0–5 or `null` to clear. Each rating is written to the
asset's `.albuminfo.xmp` sidecar on a best-effort basis, persisted, and the
in-memory tree is rebuilt before the call returns.

Corner cases: a rating above 5 is rejected before any asset is touched, so
one bad value fails the whole batch. An index whose asset record no longer
exists is skipped and still answers 200, and a failed sidecar write is
logged rather than failing the request.

Errors: 400 `rating` is above 5, or the body cannot be parsed — 401 missing
or invalid admin credentials; share tokens are not accepted — 405 read-only
mode — 500 unknown snapshot, out-of-range index, or storage failure.

> Body parameter

```json
{
  "indexArray": [0],
  "rating": 0,
  "timestamp": 0
}
```

<h3 id="rate-the-listed-assets-0–5,-or-clear-the-rating.-parameters">Parameters</h3>

| Name | In   | Type                                    | Required | Description |
| ---- | ---- | --------------------------------------- | -------- | ----------- |
| body | body | [EditRatingData](#schemaeditratingdata) | true     | none        |

<h3 id="rate-the-listed-assets-0–5,-or-clear-the-rating.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Rating updated                                                                             | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Add and remove tags on the listed assets and return the resulting tag list.

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

`indexArray` addresses the assets in the snapshot named by `timestamp`, and
additions and removals are applied per asset in list order after
NFC-normalization and filtering to valid XML characters. Each asset's
`.albuminfo.xmp` sidecar is rewritten on a best-effort basis, and the
returned list counts every tag over the whole metadata table, not only the
edited assets.

Corner cases: a tag that sanitizes to empty is skipped on add but still
serves as the removal key, and an index whose asset record no longer exists
is skipped and still answers 200.

Errors: 400 malformed body — 401 missing or invalid admin credentials; share
tokens are not accepted — 405 read-only mode — 500 unknown snapshot,
out-of-range index, or storage failure.

> Body parameter

```json
{
  "addTagsArray": ["string"],
  "indexArray": [0],
  "removeTagsArray": ["string"],
  "timestamp": 0
}
```

<h3 id="add-and-remove-tags-on-the-listed-assets-and-return-the-resulting-tag-list.-parameters">Parameters</h3>

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

<h3 id="add-and-remove-tags-on-the-listed-assets-and-return-the-resulting-tag-list.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Tags updated                                                                               | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<h3 id="add-and-remove-tags-on-the-listed-assets-and-return-the-resulting-tag-list.-responseschema">Response Schema</h3>

Status Code **200**

| Name        | Type                        | Required | Restrictions | Description |
| ----------- | --------------------------- | -------- | ------------ | ----------- |
| _anonymous_ | [[TagInfo](#schemataginfo)] | false    | none         | none        |
| » number    | integer                     | true     | none         | none        |
| » tag       | string                      | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## Replace an asset's compressed thumbnail with an uploaded frame.

<a id="opIdregenerate_thumbnail_with_frame"></a>

> Code samples

```shell
# You can also use wget
curl -X PUT /put/regenerate-thumbnail-with-frame \
  -H 'Content-Type: multipart/form-data'

```

```http
PUT /put/regenerate-thumbnail-with-frame HTTP/1.1

Content-Type: multipart/form-data

```

```javascript
const inputBody = "{}";
const headers = {
  "Content-Type": "multipart/form-data",
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
  'Content-Type' => 'multipart/form-data'
}

result = RestClient.put '/put/regenerate-thumbnail-with-frame',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'multipart/form-data'
}

r = requests.put('/put/regenerate-thumbnail-with-frame', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'multipart/form-data',
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
        "Content-Type": []string{"multipart/form-data"},
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

The multipart body carries the `asset_id` and the `frame` file, and the
frame is written to the asset's compressed-thumbnail path,
`object/compressed/<first two characters of the asset id>/<asset_id>.jpg`,
overwriting whatever was there. `thumbhash`, `phash` and `updatedAt` are
recomputed and flushed.

Corner cases: the frame is written before the record is looked up, so a
request naming an unknown id still leaves the file behind. The recomputed
hashes are derived from the original file on disk rather than from the
frame, and the stored dimensions, EXIF and `.albuminfo.xmp` sidecar are
left untouched.

Errors: 400 malformed multipart body or an `asset_id` longer than 64 bytes —
401 missing or invalid admin credentials; share tokens are not accepted —
404 `asset_id` names no asset record — 405 read-only mode — 500 frame write,
decode, or storage failure.

> Body parameter

```yaml
{}
```

<h3 id="replace-an-asset's-compressed-thumbnail-with-an-uploaded-frame.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description |
| ---- | ---- | ------ | -------- | ----------- |
| body | body | object | true     | none        |

<h3 id="replace-an-asset's-compressed-thumbnail-with-an-uploaded-frame.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Thumbnail regenerated                                                                      | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                                                                  | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Rotate an image 90° counter-clockwise and rebuild its derivatives.

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
  "asset_id": "string"
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

The compressed JPEG is loaded, rotated and written back, `width` and
`height` are swapped, and `thumbhash` and `phash` are recomputed. Each
parent album's `updatedAt` is bumped before the records are flushed.

Corner cases: only the compressed JPEG is re-rendered — the original file,
its EXIF orientation and the `.albuminfo.xmp` sidecar are left as they were,
so the rotation is a display-level transform that accumulates across
repeated calls.

Errors: 400 an `asset_id` longer than 64 bytes, or an album or video — 401
missing or invalid admin credentials; share tokens are not accepted — 404
`asset_id` names no asset record — 405 read-only mode — 500 decode,
thumbnail write, or storage failure.

> Body parameter

```json
{
  "asset_id": "string"
}
```

<h3 id="rotate-an-image-90°-counter-clockwise-and-rebuild-its-derivatives.-parameters">Parameters</h3>

| Name | In   | Type                                            | Required | Description |
| ---- | ---- | ----------------------------------------------- | -------- | ----------- |
| body | body | [RotateImageRequest](#schemarotateimagerequest) | true     | none        |

<h3 id="rotate-an-image-90°-counter-clockwise-and-rebuild-its-derivatives.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Image rotated                                                                              | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                                                                  | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-config">config</h1>

Server configuration: read, write, password, export/import, path completion

## Serve the client-visible server configuration.

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

Returns the values a client needs to drive the other operations: address
and port, the image library root (`imagePath`), `uploadFolder`,
`maxUploadSize`, `readOnlyMode`, `disableImg`, `fsNotifyWatcher`,
`normalizeUploadFilenames`, `validateUploadContent`,
`useClientTimestampInfo`, plus `hasPassword` and `hasAuthKey`.

Corner cases: secrets are never returned — `hasPassword` and `hasAuthKey`
only report whether a password or an auth key is configured. `imagePath` is
the absolute library root and serializes as `null` when it is unset.

Errors: 400 half-supplied share credentials or an id that is not an album —
401 no valid admin or share credentials.

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

<h3 id="serve-the-client-visible-server-configuration.-responses">Responses</h3>

| Status | Meaning                                                          | Description                                                                                | Schema                                  |
| ------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Public configuration                                                                       | [ConfigResponse](#schemaconfigresponse) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input                                                                              | None                                    |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)  | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                    |

<aside class="success">
This operation does not require authentication
</aside>

## Export the full server configuration as JSON.

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

Returns the complete serialized configuration, including `password` and
`authKey` in plaintext whenever they are set, so the response has to be
handled as a secret. `webRoot` is the only field never serialized.

Errors: 400 invalid input — 401 no valid admin credentials.

> Example responses

> 200 Response

```
"string"
```

<h3 id="export-the-full-server-configuration-as-json.-responses">Responses</h3>

| Status | Meaning                                                          | Description                                                                                | Schema |
| ------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Exported configuration                                                                     | string |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)  | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Complete a filesystem directory path for the path-completion field.

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

An absent or blank `path` returns the platform roots plus the
subdirectories of the server's working directory with `is_default: true`. A
trailing separator lists that directory, a bare name is matched against the
roots and the working directory, and a path with a parent lists the parent
filtered by its last segment.

Corner cases: only directories are returned, as absolute paths, sorted
case-insensitively and truncated to 50 entries. A non-empty prefix that
matches nothing is a 404, while a prefix-less listing that finds nothing
answers 200 with no children.

Errors: 400 invalid input — 401 no valid admin credentials — 404 the prefix
matched no directory — 500 the directory could not be read.

<h3 id="complete-a-filesystem-directory-path-for-the-path-completion-field.-parameters">Parameters</h3>

| Name | In    | Type   | Required | Description                                                                              |
| ---- | ----- | ------ | -------- | ---------------------------------------------------------------------------------------- |
| path | query | string | false    | Directory path prefix to complete; absent or blank lists the roots and working directory |

> Example responses

> 200 Response

```json
{
  "children": ["string"],
  "is_default": true,
  "roots": ["string"]
}
```

<h3 id="complete-a-filesystem-directory-path-for-the-path-completion-field.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                              |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Filesystem path completion                                                                 | [FsCompletion](#schemafscompletion) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None                                |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                                                                  | None                                |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                |

<aside class="success">
This operation does not require authentication
</aside>

## Replace the server configuration with the posted one.

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

The body is a complete `AppConfig` rather than a patch. Before writing,
`imagePath` is trimmed of whitespace and quotes (an empty value becomes
`null`), `uploadFolder` is trimmed, and an empty `authKey` is stored as
`null`, moving JWT signing onto the random secret generated once per
process. On success `config.toml` is rewritten, the in-memory configuration
is replaced and the filesystem watcher reloaded.

Corner cases: The posted configuration is accepted as-is: unlike
`PUT /put/config` it applies no validation, so an `uploadFolder` that is
absolute or contains `..` is stored here. A failed write is a 500 and leaves
the running configuration untouched.

Errors: 400 unusable request body — 401 missing or invalid credentials —
500 storage failure.

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

<h3 id="replace-the-server-configuration-with-the-posted-one.-parameters">Parameters</h3>

| Name | In   | Type                          | Required | Description |
| ---- | ---- | ----------------------------- | -------- | ----------- |
| body | body | [AppConfig](#schemaappconfig) | true     | none        |

<h3 id="replace-the-server-configuration-with-the-posted-one.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Config imported                                                                            | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Patch the server configuration with the posted fields.

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

Every field is optional and a field absent from the body keeps its current
value. The only validation is on `uploadFolder`, which must be a relative
path free of `..` components; on success `config.toml` is rewritten, the
in-memory configuration is replaced and the filesystem watcher is reloaded.

Corner cases: `authKey` is trimmed, and clearing or replacing it moves JWT
signing onto a new key, invalidating every token issued under the previous
one. `uploadFolder` and `maxUploadSize` take an empty string to reset to
their defaults, `uploads` and `100MiB`. `address` and `port` are stored but
the listener is not rebound, so they take effect only after a restart.

Errors: 400 `uploadFolder` is absolute or contains `..`, or the body cannot
be parsed — 401 missing or invalid admin credentials; share tokens are not
accepted — 405 read-only mode — 500 config write failure.

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

<h3 id="patch-the-server-configuration-with-the-posted-fields.-parameters">Parameters</h3>

| Name | In   | Type                                                            | Required | Description |
| ---- | ---- | --------------------------------------------------------------- | -------- | ----------- |
| body | body | [PartialUpdateConfigRequest](#schemapartialupdateconfigrequest) | true     | none        |

<h3 id="patch-the-server-configuration-with-the-posted-fields.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Config updated                                                                             | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Change or clear the account password.

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

`oldPassword` is compared verbatim against the stored password, and
`password` supplies the new one, which is trimmed before it is stored. Only
the password moves — `authKey` is untouched — so tokens already signed with
it stay valid across the change.

Corner cases: an omitted or blank `password` clears the password, after
which any input to the sign-in operation is accepted. Because `oldPassword`
is compared verbatim against the stored value, it must be omitted rather
than sent empty while no password is set.

Errors: 400 `oldPassword` does not match the stored password, or the body
cannot be parsed — 401 missing or invalid admin credentials; share tokens
are not accepted — 405 read-only mode — 500 config write failure.

> Body parameter

```json
{
  "oldPassword": "string",
  "password": "string"
}
```

<h3 id="change-or-clear-the-account-password.-parameters">Parameters</h3>

| Name | In   | Type                                                  | Required | Description |
| ---- | ---- | ----------------------------------------------------- | -------- | ----------- |
| body | body | [UpdatePasswordRequest](#schemaupdatepasswordrequest) | true     | none        |

<h3 id="change-or-clear-the-account-password.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Password updated                                                                           | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-index">index</h1>

Filesystem indexing jobs and full rebuild

## Report the state of the most recently started album-indexing job.

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

The body carries the job's state (`idle`, `running`, `completed`,
`canceled` or `failed`), the root it walks, the `scanned` / `matched` /
`processed` / `failed` counters, `startedAt` / `finishedAt` epoch
milliseconds and `cancelRequested`, so an album-index run can be polled
until it settles.

Corner cases: before the first run the state is `idle` with zeroed counters
and no timestamps. Only the album-index job writes this status: a new run
overwrites it, and single-image indexing never appears here.

Errors: 401 no valid admin credentials.

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

<h3 id="report-the-state-of-the-most-recently-started-album-indexing-job.-responses">Responses</h3>

| Status | Meaning                                                         | Description                                                                                | Schema                                      |
| ------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)         | Album index status                                                                         | [AlbumIndexStatus](#schemaalbumindexstatus) |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                        |

<aside class="success">
This operation does not require authentication
</aside>

## Index every media file under a directory tree in the background.

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

`album` is a directory path relative to `IMAGE_HOME`, where `"/"` selects
the root. The walk is asynchronous: the response returns once the job is
accepted, and progress — the scanned, matched, processed and failed
counters plus the final state — is reported by `GET /get/index/status`.

Corner cases: One album-index job runs at a time, so a request made while
another job is running is a 409. The walk does not stop at a file it cannot
read or decode; it counts the failure and carries on.

Errors: 400 no `imagePath` configured, a path that is missing or not a
directory, or a Picasu internal data directory — 401 missing or invalid
credentials — 405 read-only mode — 409 an index job is already running.

> Body parameter

```json
{
  "album": "string"
}
```

<h3 id="index-every-media-file-under-a-directory-tree-in-the-background.-parameters">Parameters</h3>

| Name | In   | Type                                          | Required | Description |
| ---- | ---- | --------------------------------------------- | -------- | ----------- |
| body | body | [IndexAlbumRequest](#schemaindexalbumrequest) | true     | none        |

<h3 id="index-every-media-file-under-a-directory-tree-in-the-background.-responses">Responses</h3>

| Status | Meaning                                                                 | Description                                                                                | Schema |
| ------ | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 202    | [Accepted](https://tools.ietf.org/html/rfc7231#section-6.3.3)           | Album indexing started                                                                     | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)        | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)         | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5) | Read-only mode                                                                             | None   |
| 409    | [Conflict](https://tools.ietf.org/html/rfc7231#section-6.5.8)           | An index job is already running                                                            | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Request cancellation of the running album index job.

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

Stores a cancel flag that the running walk checks as it goes; the response
returns as soon as the flag is set, while the job itself keeps walking
until it reaches its next directory entry.

Corner cases: Cancellation is cooperative, not immediate. While the walk
winds down the job reports `cancelRequested` with its state still `running`;
the state settles as `canceled` only once the walk has actually stopped.

Errors: 400 malformed request — 401 missing or invalid credentials —
404 no index job is active.

<h3 id="request-cancellation-of-the-running-album-index-job.-responses">Responses</h3>

| Status | Meaning                                                          | Description                                                                                | Schema |
| ------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Album index cancelled                                                                      | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)  | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)   | No active index job                                                                        | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Rebuild the asset tables and metadata cache from the filesystem under `IMAGE_HOME`.

<a id="opIdrebuild_handler"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /post/rebuild \
  -H 'Accept: application/json'

```

```http
POST /post/rebuild HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/post/rebuild", {
  method: "POST",

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

result = RestClient.post '/post/rebuild',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.post('/post/rebuild', headers = headers)

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
    $response = $client->request('POST','/post/rebuild', array(
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
URL obj = new URL("/post/rebuild");
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
        "Accept": []string{"application/json"},
    }

    data := bytes.NewBuffer([]byte{jsonReq})
    req, err := http.NewRequest("POST", "/post/rebuild", data)
    req.Header = headers

    client := &http.Client{}
    resp, err := client.Do(req)
    // ...
}

```

`POST /post/rebuild`

Clears `ASSET_BY_PATH`, `ASSET_BY_ID`, `DUPE_INDEX` and `METADATA_TABLE`,
walks the image root, and repopulates all four: identity from the walk,
metadata through the same pipeline the incremental indexer runs. The
response carries the rebuild stats and is returned only after an
in-memory tree refresh, so it does not race a following prefetch or
get-data call.

Corner cases: Rows are rebuilt rather than merged: the rebuild assigns
new `asset_id`s, so rows keyed by the previous ids must not remain. A
file whose metadata pipeline fails keeps its identity, logs the failure,
and does not stop the walk.

Errors: 400 malformed request — 401 missing or invalid credentials —
405 read-only mode — 500 `imagePath` unset, or a failure while walking the
image root or writing the tables.

> Example responses

> 200 Response

```json
{
  "albumsCreated": 0,
  "hashErrors": 0,
  "mediaCreated": 0,
  "unsupportedSkipped": 0
}
```

<h3 id="rebuild-the-asset-tables-and-metadata-cache-from-the-filesystem-under-`image_home`.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                              |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Rebuild complete                                                                           | [RebuildStats](#schemarebuildstats) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None                                |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None                                |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-serving">serving</h1>

Media byte delivery (compressed and original files)

## Serve the compressed thumbnail or preview of a hashed asset.

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

Resolves the requested path under `DATA_HOME/object/compressed` and serves
`.jpg` in a single response or `.mp4` as a range-capable stream pinned to
`video/mp4`.

Corner cases: one image-serving token authorizes exactly one file — its
`hash` claim must equal the id in the last path segment. Any extension other
than `.jpg` or `.mp4`, and a path without an extension, are rejected as
invalid input.

Errors: 400 unsupported or missing file extension — 401 no valid admin or
share credentials, or no image-serving token for this file — 500 the
compressed file could not be opened.

<h3 id="serve-the-compressed-thumbnail-or-preview-of-a-hashed-asset.-parameters">Parameters</h3>

| Name      | In   | Type   | Required | Description                                                                            |
| --------- | ---- | ------ | -------- | -------------------------------------------------------------------------------------- |
| file_path | path | string | true     | Path of the file below DATA_HOME/object/compressed; the extension must be .jpg or .mp4 |

<h3 id="serve-the-compressed-thumbnail-or-preview-of-a-hashed-asset.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Compressed file                                                                            | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the original file from its current location under `imagePath`.

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

There is no copy under `DATA_HOME`: `imagePath` holds the single
authoritative copy. The last path segment is `<id>.<ext>`, where `id` is the
asset ID, and the record for that ID names the location the file is streamed
from.

Corner cases: resolving is by asset ID alone — there is no hash fallback —
and the image-serving token must both grant original access and name this
asset in its `asset_id` claim.

Errors: 400 the path carries no usable asset id — 401 no valid admin or
share credentials, or no image-serving token granting original access —
404 unknown asset id — 500 the asset record or the file could not be read.

<h3 id="serve-the-original-file-from-its-current-location-under-`imagepath`.-parameters">Parameters</h3>

| Name      | In   | Type   | Required | Description                                                          |
| --------- | ---- | ------ | -------- | -------------------------------------------------------------------- |
| file_path | path | string | true     | Path of the original below imagePath; the last segment is <id>.<ext> |

<h3 id="serve-the-original-file-from-its-current-location-under-`imagepath`.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Imported original file                                                                     | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                                                                  | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-timeline">timeline</h1>

Grid/list data: prefetch, rows, scrollbar, tag list, export

## Serve one page of timeline/list rows for a snapshot timestamp.

<a id="opIdget_data"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-data?timestamp=0&start=0&end=0 \
  -H 'Accept: application/json'

```

```http
GET /get/get-data?timestamp=0&start=0&end=0 HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-data?timestamp=0&start=0&end=0", {
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
  'timestamp' => 'integer(int64)',
'start' => 'integer',
'end' => 'integer'
}, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-data', params={
  'timestamp': '0',  'start': '0',  'end': '0'
}, headers = headers)

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
URL obj = new URL("/get/get-data?timestamp=0&start=0&end=0");
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

Returns the rows in the half-open index range `[start, end)` of the snapshot
named by `timestamp`. Media rows carry identity and timing fields only, so
tags, EXIF, description and rating come from the single-asset metadata
operation; album rows additionally carry their stored title, cover and
counts, which their tiles render.

Corner cases: `end` is clamped to the snapshot length, and a range that
starts past it answers with an empty array rather than an error. Each row's
timestamp is the snapshot's own sort date, so a media row keeps its
EXIF-derived date.

Errors: 400 unknown or expired snapshot id — 401 missing, invalid, or
mismatched prefetch token — 500 the snapshot or an asset record could not be
read.

<h3 id="serve-one-page-of-timeline/list-rows-for-a-snapshot-timestamp.-parameters">Parameters</h3>

| Name      | In    | Type           | Required | Description                                                        |
| --------- | ----- | -------------- | -------- | ------------------------------------------------------------------ |
| timestamp | query | integer(int64) | true     | Snapshot to read, identified by the timestamp returned by prefetch |
| start     | query | integer        | true     | First row index of the requested range (inclusive)                 |
| end       | query | integer        | true     | Row index to stop at (exclusive), clamped to the snapshot length   |

> Example responses

> 200 Response

```json
[
  {
    "abstractData": {},
    "assetId": "string",
    "coverHash": "string",
    "timestamp": 0,
    "token": "string"
  }
]
```

<h3 id="serve-one-page-of-timeline/list-rows-for-a-snapshot-timestamp.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Data by timestamp range                                                                    | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<h3 id="serve-one-page-of-timeline/list-rows-for-a-snapshot-timestamp.-responseschema">Response Schema</h3>

Status Code **200**

| Name           | Type                                                        | Required | Restrictions | Description                                                                                                                                                               |
| -------------- | ----------------------------------------------------------- | -------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| _anonymous_    | [[DataBaseTimestampReturn](#schemadatabasetimestampreturn)] | false    | none         | none                                                                                                                                                                      |
| » abstractData | object                                                      | true     | none         | none                                                                                                                                                                      |
| » assetId      | string                                                      | true     | none         | Path-primary asset ID.                                                                                                                                                    |
| » coverHash    | string,null                                                 | false    | none         | For albums: the cover image's content hash (used for compressed<br>thumbnail URL construction and token validation). `None` for media<br>items or albums without a cover. |
| » timestamp    | integer(int64)                                              | true     | none         | none                                                                                                                                                                      |
| » token        | string                                                      | true     | none         | none                                                                                                                                                                      |

<aside class="success">
This operation does not require authentication
</aside>

## Export the whole tree as JSON.

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

Applies no album, share or trash filtering. Streams a JSON array of
`{key, value}` entries — one per stored metadata row composed with its
identity record — served as `application/octet-stream`.

Corner cases: the response starts before iteration, so rows whose identity
record is missing or unparsable are skipped silently and the array may be
incomplete; a failure to start iterating answers with an `{"error":…}` body
instead of the array.

Errors: 400 invalid input — 401 no valid admin credentials — 500 the tables
could not be opened.

<h3 id="export-the-whole-tree-as-json.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Export data as JSON                                                                        | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the display layout of one batched row of a snapshot.

<a id="opIdget_rows"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-rows?index=0&timestamp=0 \
  -H 'Accept: application/json'

```

```http
GET /get/get-rows?index=0&timestamp=0 HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-rows?index=0&timestamp=0", {
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
  'index' => 'integer',
'timestamp' => 'integer(int64)'
}, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-rows', params={
  'index': '0',  'timestamp': '0'
}, headers = headers)

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
URL obj = new URL("/get/get-rows?index=0&timestamp=0");
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

`index` addresses a batch of 20 consecutive snapshot entries. The response
reports the batch's `start` / `end` offsets plus each entry's display width
and height and nothing else — no asset data — so a virtualized grid can
reserve space before it fetches the rows themselves.

Corner cases: an `index` beyond the snapshot's last batch is a server-side
fault rather than an empty row, and the last batch of a snapshot shorter
than 20 entries is reported with `end` past the snapshot length.

Errors: 400 unknown or expired snapshot id — 401 missing, invalid, or
mismatched prefetch token — 500 the snapshot could not be read or the row
index is out of bounds.

<h3 id="serve-the-display-layout-of-one-batched-row-of-a-snapshot.-parameters">Parameters</h3>

| Name      | In    | Type           | Required | Description                                                        |
| --------- | ----- | -------------- | -------- | ------------------------------------------------------------------ |
| index     | query | integer        | true     | Batch to lay out, addressing 20 consecutive snapshot entries       |
| timestamp | query | integer(int64) | true     | Snapshot to read, identified by the timestamp returned by prefetch |

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

<h3 id="serve-the-display-layout-of-one-batched-row-of-a-snapshot.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema            |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Row data                                                                                   | [Row](#schemarow) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None              |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None              |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None              |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the scroll bar positions of a snapshot.

<a id="opIdget_scroll_bar"></a>

> Code samples

```shell
# You can also use wget
curl -X GET /get/get-scroll-bar?timestamp=0 \
  -H 'Accept: application/json'

```

```http
GET /get/get-scroll-bar?timestamp=0 HTTP/1.1

Accept: application/json

```

```javascript
const headers = {
  Accept: "application/json",
};

fetch("/get/get-scroll-bar?timestamp=0", {
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
  'timestamp' => 'integer(int64)'
}, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Accept': 'application/json'
}

r = requests.get('/get/get-scroll-bar', params={
  'timestamp': '0'
}, headers = headers)

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
URL obj = new URL("/get/get-scroll-bar?timestamp=0");
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

Returns one entry per year/month boundary in snapshot order, each naming the
`index` of the first entry in that bucket, so scroll bar positions map onto
dates without loading any rows.

Errors: 400 unknown or expired snapshot id — 401 missing, invalid, or
mismatched prefetch token.

<h3 id="serve-the-scroll-bar-positions-of-a-snapshot.-parameters">Parameters</h3>

| Name      | In    | Type           | Required | Description                                                        |
| --------- | ----- | -------------- | -------- | ------------------------------------------------------------------ |
| timestamp | query | integer(int64) | true     | Snapshot to read, identified by the timestamp returned by prefetch |

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

<h3 id="serve-the-scroll-bar-positions-of-a-snapshot.-responses">Responses</h3>

| Status | Meaning                                                          | Description                                                                                | Schema |
| ------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | Scroll bar data                                                                            | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)  | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |

<h3 id="serve-the-scroll-bar-positions-of-a-snapshot.-responseschema">Response Schema</h3>

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

## List every tag in the tree with the number of assets carrying it.

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

Counts every stored tag in the tree together with how many assets carry it,
so the result covers assets outside the snapshot or share a client happens
to be viewing.

Corner cases: the counts are accumulated in parallel, so the order of the
returned list is unspecified and not stable between calls.

Errors: 400 invalid input — 401 no valid admin credentials.

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

<h3 id="list-every-tag-in-the-tree-with-the-number-of-assets-carrying-it.-responses">Responses</h3>

| Status | Meaning                                                          | Description                                                                                | Schema |
| ------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)          | List of tags                                                                               | Inline |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1) | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)  | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |

<h3 id="list-every-tag-in-the-tree-with-the-number-of-assets-carrying-it.-responseschema">Response Schema</h3>

Status Code **200**

| Name        | Type                        | Required | Restrictions | Description |
| ----------- | --------------------------- | -------- | ------------ | ----------- |
| _anonymous_ | [[TagInfo](#schemataginfo)] | false    | none         | none        |
| » number    | integer                     | true     | none         | none        |
| » tag       | string                      | true     | none         | none        |

<aside class="success">
This operation does not require authentication
</aside>

## Evaluate a list query, snapshot its result, and return the snapshot id.

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

The optional JSON filter body selects the assets to include; results are
cached by expression, `locate` and tree version, so an identical query
reuses the snapshot that already exists. The response carries the snapshot
`timestamp`, the row count, the `locateTo` index, a new prefetch bearer
token valid for 300 seconds, and the resolved share when the query ran for
one. That token and timestamp are what the row, data and scroll bar
operations require.

Corner cases: the filter body is optional, and a body the server cannot
parse is treated as absent and selects the whole tree. For a share the
album filter is combined with the supplied expression, and a share that
hides metadata additionally rewrites the filter so metadata-derived
conditions (tag, path, camera make or model) can never match. `locate`
names the asset whose row index comes back as `locateTo`, which is `null`
when that asset is not in the result.

Errors: 400 half-supplied share credentials or an id that is not an album —
401 no valid admin or share credentials — 500 the tree could not be read or
the snapshot could not be stored.

> Body parameter

```json
null
```

<h3 id="evaluate-a-list-query,-snapshot-its-result,-and-return-the-snapshot-id.-parameters">Parameters</h3>

| Name   | In    | Type   | Required | Description                                  |
| ------ | ----- | ------ | -------- | -------------------------------------------- |
| locate | query | string | false    | Asset whose row index comes back as locateTo |
| body   | body  | any    | true     | none                                         |

> Example responses

> 200 Response

```json
{
  "prefetch": {
    "dataLength": 0,
    "locateTo": 0,
    "timestamp": 0
  },
  "resolvedShareOpt": {
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
  },
  "token": "string"
}
```

<h3 id="evaluate-a-list-query,-snapshot-its-result,-and-return-the-snapshot-id.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema                                  |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------- |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Prefetch result                                                                            | [PrefetchReturn](#schemaprefetchreturn) |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None                                    |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None                                    |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None                                    |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-upload">upload</h1>

File upload

## Upload media files into an album or the upload folder and index each one.

<a id="opIdupload"></a>

> Code samples

```shell
# You can also use wget
curl -X POST /upload \
  -H 'Content-Type: multipart/form-data'

```

```http
POST /upload HTTP/1.1

Content-Type: multipart/form-data

```

```javascript
const inputBody = "{}";
const headers = {
  "Content-Type": "multipart/form-data",
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
  'Content-Type' => 'multipart/form-data'
}

result = RestClient.post '/upload',
  params: {
  }, headers: headers

p JSON.parse(result)

```

```python
import requests
headers = {
  'Content-Type': 'multipart/form-data'
}

r = requests.post('/upload', headers = headers)

print(r.json())

```

```php
<?php

require 'vendor/autoload.php';

$headers = array(
    'Content-Type' => 'multipart/form-data',
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
        "Content-Type": []string{"multipart/form-data"},
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

With a target album the files land in that album's own directory; without
one they land in the configured `uploadFolder` below `imagePath`, which
becomes its own top-level album. Each file is written to its final location
and indexed before the response returns.

Corner cases: Credentials may be admin or share — a share is accepted when
it sets `showUpload`, is unexpired, matches `x-share-password` and names
the same album as `presigned_album_id_opt`. `on_conflict` defaults to
`rename`, which suffixes a collision `-001`, `-002`, …; `skip` discards that
part without writing or indexing it, and any other value is a 400. Every
part is validated before the first file is written, so a rejected name, an
extension outside the image or video whitelist, or a
`file`/`lastModified` count mismatch aborts the batch with a 400 and leaves
nothing behind. The stored extension always comes from the part's
`Content-Type` rather than the client filename, and a file the indexing
pipeline cannot decode is removed again and reported as a 400.

Errors: 400 unknown `on_conflict`, unsupported or mismatched file type,
rejected filename, count mismatch, missing `imagePath`, or an undecodable
file — 401 missing or invalid admin or share credentials — 405 read-only
mode — 500 storage failure.

> Body parameter

```yaml
{}
```

<h3 id="upload-media-files-into-an-album-or-the-upload-folder-and-index-each-one.-parameters">Parameters</h3>

| Name                   | In    | Type    | Required | Description                                                                                                                                                                                                                                                                                                                                                                       |
| ---------------------- | ----- | ------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| presigned_album_id_opt | query | string  | false    | Target album id (at most 64 bytes); without it files land in the configured upload folder                                                                                                                                                                                                                                                                                         |
| on_conflict            | query | string  | false    | Conflict policy for an existing target file: rename (default; suffixes -001, -002, …) or skip; any other value is a 400                                                                                                                                                                                                                                                           |
| auto_rename            | query | boolean | false    | When true (the default), uploaded filenames are sanitized automatically: forbidden characters are stripped, reserved Windows names are prefixed, and Unicode NFC normalization is applied; a name that degrades to empty falls back to 'upload', yielding an 'upload-{uuid}.{ext}' final name. When false, any file whose name cannot be kept as-is is rejected with a 400 error. |
| body                   | body  | object  | true     | none                                                                                                                                                                                                                                                                                                                                                                              |

<h3 id="upload-media-files-into-an-album-or-the-upload-folder-and-index-each-one.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                                                                | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Upload successful                                                                          | None   |
| 400    | [Bad Request](https://tools.ietf.org/html/rfc7231#section-6.5.1)           | Invalid input                                                                              | None   |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1)            | Authentication credentials are missing, malformed, expired, or invalid for this operation. | None   |
| 405    | [Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)    | Read-only mode                                                                             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                                                             | None   |

<aside class="success">
This operation does not require authentication
</aside>

<h1 id="picasu-pages">pages</h1>

SPA HTML page routes served from `router/get/get_page.rs`

## Serve the SPA shell; the client router opens the timeline.

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

Returns `index.html` — the embedded asset in a build with
`embed-frontend`, otherwise `<web_root>/index.html` from the app config —
the same shell every page route returns. The Vue Router root record
redirects `/` to `/timeline` in the browser, so no server-side redirect is
issued here.

<h3 id="serve-the-spa-shell;-the-client-router-opens-the-timeline.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the albums page.

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

Returns the shared `index.html` shell for the `albums` client route. The
album list itself comes from the albums API route, which reads the album
index, so this route performs no lookup of its own.

Corner cases: An album URL has a different shape — `/album/<album-id>` —
and is matched by the rank-11 catch-all, which verifies that the album
exists before returning the same shell.

<h3 id="serve-the-spa-shell-for-the-albums-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for an albums view path resolved by the client.

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

Returns `index.html` for `/albums/view/<path..>` at any depth. The
captured path is bound and discarded; the client router resolves the view
(`view/:assetId`) from the URL, so the server performs no lookup.

Corner cases: Every path below `/albums/view/` gets the shell, including
one the client router cannot resolve into an asset id.

<h3 id="serve-the-spa-shell-for-an-albums-view-path-resolved-by-the-client.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                           |
| ---- | ---- | ------ | -------- | ------------------------------------------------------------------------------------- |
| path | path | string | true     | View path below /albums/view/; served the SPA shell and resolved by the client router |

<h3 id="serve-the-spa-shell-for-an-albums-view-path-resolved-by-the-client.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the configuration page.

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

Returns the shared `index.html` shell for the `config` client route. The
configuration the page shows is fetched separately from `/get/config`; this
route returns the shell and no configuration data.

<h3 id="serve-the-spa-shell-for-the-configuration-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the favicon.

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

Returns `favicon.ico` from the frontend build — the embedded asset in a
build with `embed-frontend`, otherwise `<web_root>/favicon.ico` — with the
content type guessed from the file extension.

Errors: 404 frontend build without the embedded asset — 500 open of
`<web_root>/favicon.ico` failed.

<h3 id="serve-the-favicon.-responses">Responses</h3>

| Status | Meaning                                                                    | Description    | Schema |
| ------ | -------------------------------------------------------------------------- | -------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Favicon file   | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found      | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the links page.

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

Returns the shared `index.html` shell for the `links` client route. The path
is fixed and carries no parameters, so this route performs no lookup of its
own.

<h3 id="serve-the-spa-shell-for-the-links-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell where the sign-in page is rendered.

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

Returns `index.html`, the shell every page route returns. The `login`
client route renders the password prompt, which posts the entered password
to the authentication API, so this route answers an authenticated session
with the same shell it gives an anonymous one.

<h3 id="serve-the-spa-shell-where-the-sign-in-page-is-rendered.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Redirect to the sign-in page.

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

Answers 303 See Other with a `Location: /login` header and an empty body.
The client that follows the redirect receives the SPA shell from the
sign-in page route, which renders the password prompt.

<h3 id="redirect-to-the-sign-in-page.-responses">Responses</h3>

| Status | Meaning                                                        | Description        | Schema |
| ------ | -------------------------------------------------------------- | ------------------ | ------ |
| 303    | [See Other](https://tools.ietf.org/html/rfc7231#section-6.4.4) | Redirect to /login | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the service worker registration script.

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

Returns `registerSW.js` from the frontend build — the embedded asset in a
build with `embed-frontend`, otherwise `<web_root>/registerSW.js` — with the
content type guessed from the `.js` extension. The browser calls it to
register `/serviceWorker.js`.

Errors: 404 frontend build without the embedded asset — 500 open of
`<web_root>/registerSW.js` failed.

<h3 id="serve-the-service-worker-registration-script.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                        | Schema |
| ------ | -------------------------------------------------------------------------- | ---------------------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Service worker registration script | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                          | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                     | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the service worker script.

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

Returns `serviceWorker.js` from the frontend build — the embedded asset in a
build with `embed-frontend`, otherwise `<web_root>/serviceWorker.js` — with
the content type guessed from the `.js` extension. The script the browser
executes for this origin installs a `fetch` listener.

Errors: 404 frontend build without the embedded asset — 500 open of
`<web_root>/serviceWorker.js` failed.

<h3 id="serve-the-service-worker-script.-responses">Responses</h3>

| Status | Meaning                                                                    | Description           | Schema |
| ------ | -------------------------------------------------------------------------- | --------------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | Service worker script | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found             | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error        | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the settings path.

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

Returns the same `index.html` as the other page routes, from the embedded
asset or from `<web_root>/index.html`.

Corner cases: The client route table has no `/setting` record, so the shell
loads and the client has no view to render for that path.

<h3 id="serve-the-spa-shell-for-the-settings-path.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for a share path.

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

Returns `index.html` for `/share/<path..>` at any depth. The captured path is
bound and discarded; the client route splits `<albumId>-<shareId>` out of the
URL and resolves the pair through the share API, so the server performs no
lookup.

Corner cases: Every path below `/share/` gets the shell, including one the
client router cannot split into an album and share id.

<h3 id="serve-the-spa-shell-for-a-share-path.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                       |
| ---- | ---- | ------ | -------- | --------------------------------------------------------------------------------- |
| path | path | string | true     | Share path below /share/; the client router splits the album and share id from it |

<h3 id="serve-the-spa-shell-for-a-share-path.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the tags page.

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

Returns the shared `index.html` shell for the `tags` client route. The tag
list the page shows comes from the tags API route, so this route performs no
lookup of its own.

<h3 id="serve-the-spa-shell-for-the-tags-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the timeline page.

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

Returns `index.html` for the `timeline` client route, the shell every page
route returns. The timeline's rows, albums and scrollbar come from the
separate `/get/...` API routes that read the metadata database, so this
route performs no lookup of its own.

<h3 id="serve-the-spa-shell-for-the-timeline-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for a timeline view path resolved by the client.

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

Returns `index.html` for `/timeline/view/<path..>` at any depth. The
captured path is bound and discarded: the client router resolves the view
(`view/:assetId`) from the URL and then reads the asset through the API
routes, so the server performs no lookup.

Corner cases: Every path below `/timeline/view/` gets the shell, including
one the client router cannot resolve into an asset id.

<h3 id="serve-the-spa-shell-for-a-timeline-view-path-resolved-by-the-client.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                             |
| ---- | ---- | ------ | -------- | --------------------------------------------------------------------------------------- |
| path | path | string | true     | View path below /timeline/view/; served the SPA shell and resolved by the client router |

<h3 id="serve-the-spa-shell-for-a-timeline-view-path-resolved-by-the-client.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the trash page.

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

Returns the shared `index.html` shell for the `trashed` client route. The
trashed content the page shows is read through the separate API routes, so
this route performs no lookup of its own.

<h3 id="serve-the-spa-shell-for-the-trash-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for a trash view path resolved by the client.

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

Returns `index.html` for `/trashed/view/<path..>` at any depth. The captured
path is bound and discarded; the client router resolves the view
(`view/:assetId`) from the URL, so the server performs no lookup.

Corner cases: Every path below `/trashed/view/` gets the shell, including one
the client router cannot resolve into an asset id.

<h3 id="serve-the-spa-shell-for-a-trash-view-path-resolved-by-the-client.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                            |
| ---- | ---- | ------ | -------- | -------------------------------------------------------------------------------------- |
| path | path | string | true     | View path below /trashed/view/; served the SPA shell and resolved by the client router |

<h3 id="serve-the-spa-shell-for-a-trash-view-path-resolved-by-the-client.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Answer the 401 status a rejected sign-in lands on.

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

Returns the status alone — no body and no `Location` header — so the
browser renders its own error page for it.

Corner cases: Every request to this path gets that status; the SPA shell
is never returned here.

Errors: 401 always returned, whatever the request carries.

<h3 id="answer-the-401-status-a-rejected-sign-in-lands-on.-responses">Responses</h3>

| Status | Meaning                                                         | Description         | Schema |
| ------ | --------------------------------------------------------------- | ------------------- | ------ |
| 401    | [Unauthorized](https://tools.ietf.org/html/rfc7235#section-3.1) | Unauthorized status | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for the videos page.

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

Returns the shared `index.html` shell for the `videos` client route — the
embedded asset in a build with `embed-frontend`, otherwise
`<web_root>/index.html`. The video list itself comes from the separate API
routes, so this route performs no lookup of its own.

<h3 id="serve-the-spa-shell-for-the-videos-page.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for a videos view path resolved by the client.

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

Returns `index.html` for `/videos/view/<path..>` at any depth. The captured
path is bound and discarded; the client router resolves the view
(`view/:assetId`) from the URL, so the server performs no lookup.

Corner cases: Every path below `/videos/view/` gets the shell, including one
the client router cannot resolve into an asset id.

<h3 id="serve-the-spa-shell-for-a-videos-view-path-resolved-by-the-client.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                           |
| ---- | ---- | ------ | -------- | ------------------------------------------------------------------------------------- |
| path | path | string | true     | View path below /videos/view/; served the SPA shell and resolved by the client router |

<h3 id="serve-the-spa-shell-for-a-videos-view-path-resolved-by-the-client.-responses">Responses</h3>

| Status | Meaning                                                 | Description     | Schema |
| ------ | ------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1) | SPA page (HTML) | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for a one-segment album path, 404 for anything else.

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

The one-segment matcher returns `index.html` when the captured segment starts
with `album-`; the prefix is the only server-side check and no album record
is read. Any other one-segment top-level path answers 404 with the JSON error
body the shared error type produces.

Corner cases: Multi-segment paths are left to the rank-11 catch-all. Among the
one-segment page and asset routes mounted at the same rank, this matcher comes
first in mount order, so a request for `/videos`, `/favicon.ico`,
`/registerSW.js` or `/serviceWorker.js` is answered here and 404s.

Errors: 404 captured segment does not start with `album-`.

<h3 id="serve-the-spa-shell-for-a-one-segment-album-path,-404-for-anything-else.-parameters">Parameters</h3>

| Name             | In   | Type   | Required | Description                                             |
| ---------------- | ---- | ------ | -------- | ------------------------------------------------------- |
| dynamic_album_id | path | string | true     | One-segment path served only when it starts with album- |

<h3 id="serve-the-spa-shell-for-a-one-segment-album-path,-404-for-anything-else.-responses">Responses</h3>

| Status | Meaning                                                        | Description     | Schema |
| ------ | -------------------------------------------------------------- | --------------- | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)        | SPA page (HTML) | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4) | Not found       | None   |

<aside class="success">
This operation does not require authentication
</aside>

## Serve the SPA shell for unmatched paths, verifying album paths first.

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

Returns `index.html` for every path no other route matches. A path starting
with `album/` is checked first: the remainder must be an existing album
record in the metadata database.

Corner cases: Rank 11 puts this route last, so the page routes and the API
routes take precedence over it. The album check covers the `album/` prefix
only; any other multi-segment path gets the shell without a database read.

Errors: 404 `/album/<album-id>` names no album record — 500 metadata
database read, blocking task, or shell file failure.

<h3 id="serve-the-spa-shell-for-unmatched-paths,-verifying-album-paths-first.-parameters">Parameters</h3>

| Name | In   | Type   | Required | Description                                                                                             |
| ---- | ---- | ------ | -------- | ------------------------------------------------------------------------------------------------------- |
| path | path | string | true     | Remaining unmatched path served with the SPA shell; album/ paths are checked against the database first |

<h3 id="serve-the-spa-shell-for-unmatched-paths,-verifying-album-paths-first.-responses">Responses</h3>

| Status | Meaning                                                                    | Description                                            | Schema |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------ | ------ |
| 200    | [OK](https://tools.ietf.org/html/rfc7231#section-6.3.1)                    | SPA fallback — serves index.html for Vue Router routes | None   |
| 404    | [Not Found](https://tools.ietf.org/html/rfc7231#section-6.5.4)             | Not found                                              | None   |
| 500    | [Internal Server Error](https://tools.ietf.org/html/rfc7231#section-6.6.1) | Internal error                                         | None   |

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

| Name                     | Type           | Required | Restrictions | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------ | -------------- | -------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| address                  | string         | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| authKey                  | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| dataHome                 | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| disableImg               | boolean        | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| fsNotifyWatcher          | boolean        | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| imagePath                | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| maxUploadSize            | string         | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| normalizeUploadFilenames | boolean        | false    | none         | NFC-normalize uploaded filenames so macOS NFD names collapse onto<br>their composed form. Optional (unlike the always-on sanitization tiers).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| password                 | string,null    | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| port                     | integer(int32) | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| readOnlyMode             | boolean        | true     | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| uploadFolder             | string         | false    | none         | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| useClientTimestampInfo   | boolean        | false    | none         | Trust the `lastModified` the upload client sends with each file. When<br>enabled, the value is used as the stored file modification time but<br>clamped to `[1970-01-01, now + 24h]` so a broken client clock cannot<br>push an undated file (e.g. a screenshot) into year 1970 or the distant<br>future. Disabled by default: the server uses `now()` instead of the<br>provided value, since a client's clock or timezone cannot be relied on.<br>Only affects files without embedded metadata: photos with a<br>`DateTimeOriginal` EXIF tag keep their EXIF-derived date regardless.<br>See `backend/src/router/post/post_upload.rs`<br>(`resolve_upload_timestamp`).                                                                                                                                                                                                                                                                   |
| validateUploadContent    | boolean        | false    | none         | Cross-check uploaded content against the type its declared<br>`Content-Type` implies. The declared extension yields the expected<br>format from the supported-format table; the content is then identified<br>and the two are compared. See `backend/src/process/format.rs`.<br><br>Content that cannot be identified at all is always rejected, with or<br>without this setting: there is no way to tell whether such bytes are<br>what the extension claims. This setting governs only the mismatch case,<br>where the content _is_ identified but is not the type the extension<br>claims — for example a video uploaded with an image content type, or an<br>image named with the wrong extension. Enabled (the default) rejects<br>those with `400 InvalidInput`; disabling it stores the file anyway, which<br>misdescribes it and leaves the indexer trusting a wrong extension. Only<br>disable this to tolerate mislabeled files. |

<h2 id="tocS_AssignAlbumData">AssignAlbumData</h2>
<!-- backwards compatibility -->
<a id="schemaassignalbumdata"></a>
<a id="schema_AssignAlbumData"></a>
<a id="tocSassignalbumdata"></a>
<a id="tocsassignalbumdata"></a>

```json
{
  "albumId": "string",
  "assetId": "string",
  "onConflict": "skip"
}
```

Request body for `PUT /put/assign_album`. Strict: unknown fields (including
the legacy multi-alias `alias` path) are rejected rather than ignored.

### Properties

| Name       | Type                            | Required | Restrictions | Description                                                                                                                                                                                                                                                                                              |
| ---------- | ------------------------------- | -------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| albumId    | string                          | true     | none         | Destination album ID; must be a filesystem-backed directory album<br>(manual albums are rejected with 400).                                                                                                                                                                                              |
| assetId    | string                          | true     | none         | Path-primary asset ID. The handler resolves the record and its<br>physical path via `ASSET_BY_ID`, allowing independent<br>movement of same-hash files.                                                                                                                                                  |
| onConflict | [OnConflict](#schemaonconflict) | true     | none         | Filename-collision strategy for moves and uploads: `skip` leaves an<br>existing destination untouched (the source stays put, outcome `skipped`);<br>`rename` lands the file under a unique suffixed name (outcome<br>`renamedFrom`). Required on assign with no default; upload defaults to<br>`rename`. |

<h2 id="tocS_AssignOutcome">AssignOutcome</h2>
<!-- backwards compatibility -->
<a id="schemaassignoutcome"></a>
<a id="schema_AssignOutcome"></a>
<a id="tocSassignoutcome"></a>
<a id="tocsassignoutcome"></a>

```json
"moved"
```

The concrete result of a successful assign: `moved`, `renamedFrom` (an
auto-`-001` suffix collision), or `skipped` (destination already exists and
strategy is skip).

### Properties

| Name        | Type   | Required | Restrictions | Description                                                                                                                                                                 |
| ----------- | ------ | -------- | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| _anonymous_ | string | false    | none         | The concrete result of a successful assign: `moved`, `renamedFrom` (an<br>auto-`-001` suffix collision), or `skipped` (destination already exists and<br>strategy is skip). |

#### Enumerated Values

| Property    | Value       |
| ----------- | ----------- |
| _anonymous_ | moved       |
| _anonymous_ | renamedFrom |
| _anonymous_ | skipped     |

<h2 id="tocS_AssignResult">AssignResult</h2>
<!-- backwards compatibility -->
<a id="schemaassignresult"></a>
<a id="schema_AssignResult"></a>
<a id="tocSassignresult"></a>
<a id="tocsassignresult"></a>

```json
{
  "outcome": "moved"
}
```

Outcome of an `assign_album` call, reported to the caller so the UI is never
silent about what happened to the selected item.

### Properties

| Name    | Type                                  | Required | Restrictions | Description                                                                                                                                                                 |
| ------- | ------------------------------------- | -------- | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| outcome | [AssignOutcome](#schemaassignoutcome) | true     | none         | The concrete result of a successful assign: `moved`, `renamedFrom` (an<br>auto-`-001` suffix collision), or `skipped` (destination already exists and<br>strategy is skip). |

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
  "assetId": "string",
  "coverHash": "string",
  "timestamp": 0,
  "token": "string"
}
```

### Properties

| Name         | Type           | Required | Restrictions | Description                                                                                                                                                               |
| ------------ | -------------- | -------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| abstractData | object         | true     | none         | none                                                                                                                                                                      |
| assetId      | string         | true     | none         | Path-primary asset ID.                                                                                                                                                    |
| coverHash    | string,null    | false    | none         | For albums: the cover image's content hash (used for compressed<br>thumbnail URL construction and token validation). `None` for media<br>items or albums without a cover. |
| timestamp    | integer(int64) | true     | none         | none                                                                                                                                                                      |
| token        | string         | true     | none         | none                                                                                                                                                                      |

<h2 id="tocS_DeleteList">DeleteList</h2>
<!-- backwards compatibility -->
<a id="schemadeletelist"></a>
<a id="schema_DeleteList"></a>
<a id="tocSdeletelist"></a>
<a id="tocsdeletelist"></a>

```json
{
  "assetIds": ["string"],
  "timestamp": 0
}
```

### Properties

| Name      | Type           | Required | Restrictions | Description                                                                                                                               |
| --------- | -------------- | -------- | ------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| assetIds  | [string]       | true     | none         | Asset IDs to delete. Each asset is resolved via `ASSET_BY_ID` by its<br>`asset_id` key. The asset file and sidecar are removed from disk. |
| timestamp | integer(int64) | true     | none         | none                                                                                                                                      |

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
  "isTrashed": true,
  "timestamp": 0
}
```

### Properties

| Name       | Type           | Required | Restrictions | Description |
| ---------- | -------------- | -------- | ------------ | ----------- |
| indexArray | [integer]      | true     | none         | none        |
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

<h2 id="tocS_OnConflict">OnConflict</h2>
<!-- backwards compatibility -->
<a id="schemaonconflict"></a>
<a id="schema_OnConflict"></a>
<a id="tocSonconflict"></a>
<a id="tocsonconflict"></a>

```json
"skip"
```

Filename-collision strategy for moves and uploads: `skip` leaves an
existing destination untouched (the source stays put, outcome `skipped`);
`rename` lands the file under a unique suffixed name (outcome
`renamedFrom`). Required on assign with no default; upload defaults to
`rename`.

### Properties

| Name        | Type   | Required | Restrictions | Description                                                                                                                                                                                                                                                                                              |
| ----------- | ------ | -------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| _anonymous_ | string | false    | none         | Filename-collision strategy for moves and uploads: `skip` leaves an<br>existing destination untouched (the source stays put, outcome `skipped`);<br>`rename` lands the file under a unique suffixed name (outcome<br>`renamedFrom`). Required on assign with no default; upload defaults to<br>`rename`. |

#### Enumerated Values

| Property    | Value  |
| ----------- | ------ |
| _anonymous_ | skip   |
| _anonymous_ | rename |

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
  "resolvedShareOpt": {
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
  },
  "token": "string"
}
```

### Properties

| Name             | Type                        | Required | Restrictions | Description |
| ---------------- | --------------------------- | -------- | ------------ | ----------- |
| prefetch         | [Prefetch](#schemaprefetch) | true     | none         | none        |
| resolvedShareOpt | any                         | false    | none         | none        |

oneOf

| Name          | Type                                  | Required | Restrictions | Description |
| ------------- | ------------------------------------- | -------- | ------------ | ----------- |
| » _anonymous_ | [ResolvedShare](#schemaresolvedshare) | false    | none         | none        |

xor

| Name          | Type | Required | Restrictions | Description |
| ------------- | ---- | -------- | ------------ | ----------- |
| » _anonymous_ | null | false    | none         | none        |

continued

| Name  | Type   | Required | Restrictions | Description |
| ----- | ------ | -------- | ------------ | ----------- |
| token | string | true     | none         | none        |

<h2 id="tocS_RebuildStats">RebuildStats</h2>
<!-- backwards compatibility -->
<a id="schemarebuildstats"></a>
<a id="schema_RebuildStats"></a>
<a id="tocSrebuildstats"></a>
<a id="tocsrebuildstats"></a>

```json
{
  "albumsCreated": 0,
  "hashErrors": 0,
  "mediaCreated": 0,
  "unsupportedSkipped": 0
}
```

Statistics from a clean filesystem rebuild.

### Properties

| Name               | Type    | Required | Restrictions | Description |
| ------------------ | ------- | -------- | ------------ | ----------- |
| albumsCreated      | integer | true     | none         | none        |
| hashErrors         | integer | true     | none         | none        |
| mediaCreated       | integer | true     | none         | none        |
| unsupportedSkipped | integer | true     | none         | none        |

<h2 id="tocS_RenewHashToken">RenewHashToken</h2>
<!-- backwards compatibility -->
<a id="schemarenewhashtoken"></a>
<a id="schema_RenewHashToken"></a>
<a id="tocSrenewhashtoken"></a>
<a id="tocsrenewhashtoken"></a>

```json
{
  "expiredHashToken": "string"
}
```

### Properties

| Name             | Type   | Required | Restrictions | Description |
| ---------------- | ------ | -------- | ------------ | ----------- |
| expiredHashToken | string | true     | none         | none        |

<h2 id="tocS_RenewHashTokenReturn">RenewHashTokenReturn</h2>
<!-- backwards compatibility -->
<a id="schemarenewhashtokenreturn"></a>
<a id="schema_RenewHashTokenReturn"></a>
<a id="tocSrenewhashtokenreturn"></a>
<a id="tocsrenewhashtokenreturn"></a>

```json
{
  "token": "string"
}
```

### Properties

| Name  | Type   | Required | Restrictions | Description |
| ----- | ------ | -------- | ------------ | ----------- |
| token | string | true     | none         | none        |

<h2 id="tocS_RenewTimestampToken">RenewTimestampToken</h2>
<!-- backwards compatibility -->
<a id="schemarenewtimestamptoken"></a>
<a id="schema_RenewTimestampToken"></a>
<a id="tocSrenewtimestamptoken"></a>
<a id="tocsrenewtimestamptoken"></a>

```json
{
  "token": "string"
}
```

### Properties

| Name  | Type   | Required | Restrictions | Description |
| ----- | ------ | -------- | ------------ | ----------- |
| token | string | true     | none         | none        |

<h2 id="tocS_RenewTimestampTokenReturn">RenewTimestampTokenReturn</h2>
<!-- backwards compatibility -->
<a id="schemarenewtimestamptokenreturn"></a>
<a id="schema_RenewTimestampTokenReturn"></a>
<a id="tocSrenewtimestamptokenreturn"></a>
<a id="tocsrenewtimestamptokenreturn"></a>

```json
{
  "token": "string"
}
```

### Properties

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
  "asset_id": "string"
}
```

### Properties

| Name     | Type   | Required | Restrictions | Description                     |
| -------- | ------ | -------- | ------------ | ------------------------------- |
| asset_id | string | true     | none         | Asset ID of the image to rotate |

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
  "coverAssetId": "string"
}
```

Payload for updating a specific album's cover image.

### Properties

| Name         | Type   | Required | Restrictions | Description                                  |
| ------------ | ------ | -------- | ------------ | -------------------------------------------- |
| albumId      | string | true     | none         | none                                         |
| coverAssetId | string | true     | none         | The `asset_id` of the image to set as cover. |

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
