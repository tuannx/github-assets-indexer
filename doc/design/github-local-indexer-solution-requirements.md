# GitHub Local Indexer — Solution Requirements và Design

> Trạng thái: Baseline đã chốt, chờ implementation plan
> Phiên bản: 0.2
> Phạm vi: một project độc lập, không mở rộng trực tiếp `git-bug`

## 1. Mục đích và quyết định chính

Tài liệu này định nghĩa solution requirement, kiến trúc, giới hạn MVP và tiêu chí
nghiệm thu cho một công cụ index resource từ GitHub về local để phục vụ
local-first search.

Quyết định thiết kế được đề xuất:

1. **Tách thành project độc lập**: indexer có vòng đời, schema, nhịp đồng bộ và
   yêu cầu dữ liệu khác với mô hình DAG/CRDT của `git-bug`.
2. **Local-first**: lệnh search đọc local index trước và mặc định không gọi
   network. Remote search chỉ được thực hiện khi người dùng yêu cầu rõ ràng.
3. **Rust + SQLite/FTS5 BM25**: một database local duy nhất cho metadata, sync
   state, durable job queue và full-text index. Không cần Redis, Elasticsearch,
   message broker hay service cloud cho MVP.
4. **CLI-first, async-by-default**: CLI tạo sync job bền vững rồi trả quyền
   điều khiển cho người dùng; worker chạy nền hoặc chạy foreground với
   `--wait`.
5. **Read-only ingestion trong MVP**: hệ thống đọc GitHub và cập nhật local
   index; không tạo, sửa hoặc xoá resource trên GitHub.
6. **Polling trước, webhook sau**: polling incremental đơn giản hơn, không
   yêu cầu public endpoint hay infrastructure bên ngoài. Webhook là hướng mở
   rộng, không phải điều kiện để MVP hoạt động.
7. **Staleness là trạng thái first-class**: kết quả vẫn được trả ngay cả khi
   stale, nhưng phải hiển thị rõ `remote_updated_at`, `local_synced_at`,
   freshness và sync health.
8. **Cursor Skill là integration layer**: skill chỉ gọi CLI/API local, không
   chứa logic GitHub, persistence hoặc đồng bộ.

Các quyết định trên là design proposal. Việc bắt đầu implementation cần xác
nhận lại các decision gate ở cuối tài liệu.

## 2. Bối cảnh và vấn đề

GitHub có nhiều loại resource và mỗi loại có API, quyền truy cập, pagination,
identity và semantics cập nhật khác nhau. Một cơ chế search duy nhất của GitHub
không phải là nguồn đồng bộ đầy đủ cho toàn bộ resource.

Các nhu cầu chính:

- tìm issue và comment khi offline hoặc mạng chậm;
- tìm pull request, conversation comment, review và inline review comment;
- tìm nội dung và metadata của GitHub Projects;
- tìm nội dung wiki;
- mở kết quả local tới URL gốc;
- biết dữ liệu local được đồng bộ lúc nào và có khả năng stale hay không;
- đồng bộ incremental, có thể dừng, chạy lại và tiếp tục sau crash;
- không làm mất dữ liệu local khi GitHub tạm thời không truy cập được;
- có thể index nhiều repository mà không tạo ra số lượng worker không giới hạn.

## 3. Phạm vi sản phẩm

### 3.1. Mục tiêu

### G-001 — Index local các resource được chọn

Hệ thống phải index được resource theo từng repository, theo collection độc lập,
với identity ổn định và quan hệ parent-child rõ ràng.

### G-002 — Search local có ngữ cảnh

Hệ thống phải tìm được title, body, comment và metadata quan trọng trong local
database, trả về snippet, loại resource, repository, resource cha, URL GitHub
và trạng thái freshness.

### G-003 — Đồng bộ incremental và có thể tiếp tục

Sau lần sync đầu tiên, các lần sau chỉ lấy phần có khả năng thay đổi, ghi
checkpoint sau từng đơn vị an toàn và resume được sau lỗi hoặc process restart.

### G-004 — Minh bạch về freshness

Người dùng phải phân biệt được:

- dữ liệu local còn mới;
- chưa kiểm tra remote trong thời gian cho phép;
- remote đã xác nhận có phiên bản mới hơn;
- lần sync gần nhất thất bại;
- chỉ một phần collection bị stale hoặc lỗi.

### G-005 — Bounded asynchronous work

Sync phải chạy bất đồng bộ nhưng có giới hạn về số worker, số request đồng thời,
kích thước page và bộ nhớ. Một repository lớn không được làm process tạo số
lượng async task hoặc memory không giới hạn.

### G-006 — Bảo toàn dữ liệu và quyền truy cập

Token, raw payload và nội dung private repository phải được bảo vệ ở local.
Lỗi mạng hoặc lỗi permission của một collection không được xoá dữ liệu đã
index thành công ở collection khác.

### 3.2. Không thuộc MVP

Các hạng mục sau không thuộc MVP trừ khi có quyết định riêng:

- tạo, sửa, xoá issue, PR, comment, project hoặc wiki trên GitHub;
- đồng bộ hai chiều và conflict resolution;
- realtime guarantee thông qua webhook;
- GitHub global code search hoặc index toàn bộ source code;
- Elasticsearch, OpenSearch, Redis, Kafka, cloud database hoặc remote queue;
- multi-user server, shared remote index hoặc cross-machine replication;
- render HTML/Markdown giàu tính tương tác trong indexer;
- index đầy đủ mọi GitHub resource ngay từ phiên bản đầu tiên;
- thu thập telemetry ra bên ngoài mặc định.

## 4. Người dùng và use case

### UC-001 — Thêm repository

Người dùng chạy:

```text
github-local-indexer repo add owner/name
```

Hệ thống xác thực format repository, kiểm tra credential và khả năng truy cập
metadata. Nếu repository hợp lệ, hệ thống tạo source local và enqueue initial
sync.

### UC-002 — Initial sync

Initial sync được enqueue bền vững và chạy nền khi local worker daemon đang hoạt
động, có progress và không bắt buộc giữ terminal mở. Nếu daemon chưa chạy,
người dùng có thể chạy `daemon run` hoặc dùng `sync --wait` để xử lý foreground.
Không tự tạo detached child process không được quản lý.

### UC-003 — Search khi offline

Khi không có network, `search` vẫn trả về dữ liệu đã index kèm freshness và
sync health. Search không được fail chỉ vì remote unavailable.

### UC-004 — Search có remote fallback (P1)

Người dùng có thể yêu cầu remote fallback bằng flag rõ ràng. Kết quả chỉ có ở
remote phải được đánh dấu `remote_only` và không tự động được coi là local
record nếu chưa ingest thành công.

### UC-005 — Phát hiện stale

Khi TTL freshness hết hạn, hệ thống đánh dấu collection là `possibly_stale` và
enqueue refresh theo policy. Khi API trả về phiên bản mới hơn, hệ thống chuyển
thành `confirmed_stale` cho tới khi snapshot local được cập nhật.

### UC-006 — Sync bị lỗi

Nếu sync lỗi do rate limit, network, permission hoặc payload không hợp lệ, dữ
liệu cũ vẫn có thể search. `status` phải chỉ ra collection lỗi, nguyên nhân,
thời điểm lỗi, số lần retry và thời điểm retry tiếp theo.

### UC-007 — Nhiều repository

Người dùng có thể cấu hình nhiều repository. Scheduler phải giới hạn concurrency
toàn cục và per source, không để một repository chiếm toàn bộ capacity.

## 5. Mô hình resource và phạm vi index

### 5.1. Identity và namespace

Mỗi record phải có:

- `provider = github`;
- `source_id` đại diện cho một repository đã cấu hình;
- `resource_type`;
- `remote_id` ổn định;
- `remote_node_id` nếu provider cung cấp;
- `parent_remote_id` khi resource là comment, review hoặc project item;
- canonical URL;
- `content_hash` của phần nội dung chuẩn hoá.

Khóa logic phải có dạng tương đương:

```text
(provider, source_id, resource_type, remote_id)
```

Không được dùng URL, title hoặc số thứ tự trong trang làm primary identity.

Các namespace sau phải tách biệt:

- `repository`;
- `issue`;
- `issue_comment`;
- `pull_request`;
- `pull_request_comment`;
- `pull_request_review`;
- `pull_request_review_comment`;
- `project`;
- `project_item`;
- `wiki_page`;
- `wiki_revision`.

`pull_request` không được gộp vào `issue` chỉ vì GitHub expose một phần PR qua
issue API. Conversation comment trên PR và inline review comment cũng phải giữ
đúng loại resource để search và hiển thị chính xác ngữ cảnh.

### 5.2. P0 — MVP bắt buộc

#### Repository

Phải index metadata cần cho filter và display:

- owner, name, full name;
- description;
- visibility;
- default branch;
- archived, fork;
- labels và milestones ở mức cần thiết cho filter;
- GitHub URL;
- remote created/updated timestamps nếu có.

#### Issues

Phải index:

- issue number và title;
- body;
- state, state reason nếu có;
- author, assignees, participants;
- labels, milestone;
- created, updated, closed timestamps;
- issue comments, gồm comment body, author, timestamps và parent issue;
- URL và raw remote identity.

#### Pull requests

Phải index:

- PR number, title, body;
- state, draft, merged và merge timestamp;
- author, assignees, reviewers;
- labels, milestone, base/head branch;
- created, updated, closed timestamps;
- conversation comments;
- reviews;
- inline review comments, gồm file path, line/side nếu có, body, author,
  timestamps và review/PR parent;
- URL và raw remote identity.

#### GitHub Projects

P0 phải index phần dữ liệu cốt lõi của Projects v2:

- project identity, title, description, owner, URL, visibility nếu có;
- fields và field options;
- project items;
- item field values;
- liên kết item tới issue, PR hoặc draft item khi API cho phép;
- timestamps và remote identity.

Project view/layout chi tiết không phải blocker của P0. Nếu provider không
truy cập được một field hoặc view do permission/API limitation, phải lưu
capability result và báo `unsupported` hoặc `permission_denied`, không báo
toàn bộ repository sync thành công giả.

#### Wiki

P0 phải index:

- wiki page identity;
- title và slug;
- Markdown/content hiện tại;
- canonical wiki URL;
- commit/revision identity hiện tại;
- author và timestamps nếu lấy được;
- page links ở mức có thể trích xuất an toàn.

GitHub Wiki nên được xử lý như một nguồn git riêng thông qua clone/fetch hoặc
adapter tương đương. Không được giả định repository API thông thường trả đủ
toàn bộ lịch sử wiki.

P0 không bắt buộc index toàn bộ wiki revision history, nhưng schema phải đủ để
mở rộng thành `wiki_revision` mà không phá vỡ identity hiện tại.

### 5.3. P1 — Mở rộng sau MVP

- GitHub Discussions và discussion comments;
- releases và release assets metadata;
- reactions;
- issue/PR timeline events;
- project views đầy đủ;
- wiki revision history;
- PR commits, changed files và check/status metadata;
- labels, milestones, users và teams như các document độc lập khi nhu cầu
  filter/search tăng lên.

### 5.4. P2 — Chỉ làm khi có requirement riêng

- source code và Git blobs;
- commit message/full diff search;
- Actions workflow/run/log;
- security alerts, Dependabot và secret scanning;
- organization-wide index;
- GitHub global search hoặc multi-provider federation.

Mỗi resource mới phải có capability contract, identity rule, incremental
strategy, permission requirement, deletion behavior và test fixtures trước khi
được đưa vào production.

## 6. Functional requirements

### 6.1. Source management

#### FR-001 — Add source

Người dùng phải thêm được một repository bằng canonical owner/name hoặc URL.
Input phải được validate ở boundary và normalize về cùng một source identity.

#### FR-002 — Multiple sources

Hệ thống phải hỗ trợ nhiều source và không trộn resource giữa các repository
trong search hoặc sync state.

#### FR-003 — Enable/disable

Source có thể bị disable để dừng job mới mà không xoá dữ liệu local.

#### FR-004 — Delete source

Xoá source phải là thao tác explicit. Hệ thống phải yêu cầu confirmation và
phải phân biệt:

- disable source, giữ data;
- remove source metadata, giữ data;
- purge source data, xoá local records và FTS documents.

MVP không tự purge dữ liệu chỉ vì remote resource đã bị xoá.

### 6.2. Ingestion

#### FR-005 — Initial sync

Initial sync phải lấy dữ liệu theo collection, page bằng cursor hoặc pagination
được provider hỗ trợ, không tải toàn bộ dataset vào memory.

#### FR-006 — Incremental sync

Mỗi collection phải có checkpoint riêng. Checkpoint được commit sau khi page
hoặc batch tương ứng đã được persist thành công.

#### FR-007 — Idempotency

Chạy lại cùng một page, job hoặc batch bất kỳ không được tạo duplicate record,
duplicate FTS document hoặc mất quan hệ parent-child.

#### FR-008 — Updates

Khi remote resource đổi title, body, state, label, author, timestamps hoặc
metadata được hỗ trợ, local snapshot và FTS document phải được cập nhật.

#### FR-009 — Deletions

Deletion phải được xử lý explicit:

- nếu endpoint trả về deleted resource, ghi `remote_deleted_at` và loại khỏi
  kết quả search mặc định;
- nếu endpoint không cung cấp deletion event, chỉ full reconciliation mới được
  phép kết luận resource đã bị xoá;
- không xoá local record ngay khi một request tạm thời không trả về record.

#### FR-010 — Partial failure

Issue comments lỗi không được làm mất issue đã sync thành công. Project lỗi
không được rollback issue/PR collection không liên quan.

#### FR-011 — Capability-aware sync

Sync phải ghi nhận capability state riêng cho từng collection:

- `enabled`;
- `unsupported`;
- `permission_denied`;

Đồng thời phải ghi nhận sync outcome/health riêng:

- `idle`;
- `running`;
- `failed`;
- `blocked`;
- `succeeded`.

`rate_limited` là error category/scheduling state, không phải capability state.

Một source chỉ được báo `fully_synced` khi mọi collection bắt buộc đã thành
công hoặc được người dùng chấp nhận là không khả dụng.

### 6.3. Async jobs

#### FR-012 — Durable queue

Job phải được lưu trong SQLite trước khi CLI trả về. Process restart không được
làm mất job đã enqueue.

#### FR-013 — Bounded workers

Worker pool phải có:

- global concurrency limit;
- per-source concurrency limit;
- per-token/provider rate limiter;
- page size và payload size limit;
- cancellation token và deadline.

Không tạo một async task cho mỗi resource. Worker xử lý theo page/batch và giải
phóng dữ liệu trước khi chuyển sang batch tiếp theo.

#### FR-014 — Job lease và recovery

Job `running` phải có lease/heartbeat. Job hết lease phải được reclaim an toàn
và chạy lại từ checkpoint gần nhất.

#### FR-015 — Retry

Chỉ retry tự động với lỗi transient:

- timeout, connection reset, DNS tạm thời;
- HTTP 429 theo `Retry-After`;
- HTTP 5xx.

Không retry vô hạn. Lỗi token, permission, malformed request hoặc resource
không tồn tại phải kết thúc job với error phân loại rõ ràng.

Retry phải dùng exponential backoff có jitter và có upper bound.

#### FR-016 — Cancellation

Người dùng phải cancel được job. Cancellation không được rollback các transaction
đã commit và không được đánh dấu checkpoint cho page chưa persist xong.

### 6.4. Search

#### FR-017 — Local-first default

`search` mặc định chỉ đọc SQLite/FTS5. Không có hidden network request.

#### FR-018 — Full-text scope

FTS P0 phải tìm được:

- title;
- body/content;
- issue/PR conversation comment;
- review body;
- inline review comment;
- wiki content;
- author/label/branch text khi phù hợp.

Comment và inline comment là document độc lập nhưng phải trả parent context.

#### FR-019 — Filters

Search P0 phải hỗ trợ tối thiểu:

- repository/source;
- resource type;
- state;
- author;
- assignee/reviewer;
- label;
- milestone;
- project;
- updated time;
- freshness/sync health.

Query language nên dùng cú pháp qualifier đơn giản, có parser riêng và test
được. Không cần clone toàn bộ cú pháp GitHub search trong MVP.

#### FR-020 — Result contract

Mỗi result phải có:

- stable local ID;
- provider/resource type;
- repository;
- title hoặc content excerpt;
- parent resource nếu có;
- GitHub URL;
- `remote_updated_at`;
- `local_synced_at`;
- freshness;
- sync health;
- rank/score nếu search engine cung cấp.

#### FR-021 — Remote fallback explicit (P1)

`--remote` hoặc option tương đương mới được phép gọi GitHub. Kết quả remote-only
phải có trạng thái riêng và không được làm local search trông như đã index đầy
đủ.

#### FR-022 — Search consistency

Không được trả về FTS document trỏ tới record đã bị purge. Upsert resource,
update/delete FTS document và quan hệ chính phải nằm trong cùng local
transaction hoặc có reconciliation để khôi phục invariant.

### 6.5. Freshness, stale và notification

#### FR-023 — Separate state axes

Không dùng một enum duy nhất để biểu diễn mọi tình huống. Hệ thống phải tách ít
nhất hai trục:

`freshness`:

- `unknown`;
- `fresh`;
- `possibly_stale`;
- `confirmed_stale`;
- `partial_stale`.

`sync_health`:

- `never_synced`;
- `idle`;
- `running`;
- `failed`;
- `blocked`.

Một collection có thể đồng thời là `confirmed_stale` và `failed`; mô hình này
tránh làm mất thông tin khi sync refresh thất bại.

#### FR-024 — Timestamps

Phải lưu và hiển thị riêng:

- `remote_created_at`;
- `remote_updated_at`: thời điểm GitHub cho biết resource thay đổi;
- `local_synced_at`: thời điểm snapshot đó được persist local;
- `last_checked_at`: thời điểm hệ thống kiểm tra remote gần nhất;
- `last_success_at`;
- `last_failure_at`.

Không được gọi `local_synced_at` là “last updated” nếu không nói rõ đó là local
sync time. UI/CLI nên hiển thị cả “Remote updated” và “Synced locally”.

#### FR-025 — Freshness policy

Freshness policy phải cấu hình được theo source hoặc collection. Policy gồm:

- TTL;
- whether to auto-refresh;
- maximum retry delay;
- whether stale notification is enabled.

Baseline proposal để benchmark:

- issue, PR, comment và review: 15 phút;
- repository metadata, project và wiki: 60 phút;
- full reconciliation: 24 giờ.

Giá trị mặc định cần được benchmark và ghi trong config reference; không hardcode
policy trong search layer.

Với workflow agent mặc định, `freshness` vẫn chuyển sang `stale` theo TTL của
collection, nhưng agent chỉ tự enqueue refresh khi tuổi của bất kỳ collection
nào vượt 1.440 phút (24 giờ). Source-level age ghi nhận lần ghi local mới nhất
nên không thay thế tuổi từng collection. `refresh_recommended` một mình không
kích hoạt enqueue. Enqueue phải trả job ID ngay, không chặn local search; agent
tiếp tục dùng kết quả local và báo rõ job đang queued. Không enqueue trùng khi
source đã có job pending/running. Explicit user sync vẫn có thể chạy ở bất kỳ
tuổi cache nào.

#### FR-026 — Staleness transition

Quy tắc tối thiểu:

- chưa từng sync thành công: `unknown`;
- `now - last_checked_at <= TTL` và không thấy remote version mới hơn:
  `fresh`;
- TTL hết nhưng chưa kiểm tra remote: `possibly_stale`;
- API trả version/updated timestamp/ETag mới hơn local: `confirmed_stale`;
- nhiều collection của source có trạng thái khác nhau: source aggregate là
  `partial_stale`.

Remote timestamp chỉ dùng làm evidence; không dùng timestamp equality làm
identity hoặc cơ chế dedupe duy nhất.

#### FR-027 — Background refresh

Khi data hết TTL, search vẫn trả local data ngay. Hệ thống có thể enqueue refresh
với debounce, nhưng không được chặn search để đợi network. Theo policy agent mặc
định, tự enqueue chỉ xảy ra khi index đã cũ hơn 24 giờ; tuổi vượt TTL ngắn hơn
vẫn được báo stale nhưng không tự enqueue. Enqueue trả job ID và không được xem
là refresh hoàn tất cho tới khi worker báo thành công.

#### FR-028 — Durable local notifications

MVP phải tạo local sync event cho:

- `stale_detected`;
- `sync_started`;
- `sync_failed`;
- `sync_recovered`;
- `sync_completed`.

Notification phải dedupe theo source, collection, condition và remote version
hoặc failure incident. Không gửi lặp vô hạn mỗi lần search.

MVP bắt buộc cung cấp event qua `status`, `jobs` và JSON output. OS desktop
notification là P1, không phải dependency của core.

## 7. Kiến trúc đề xuất

### 7.1. Sơ đồ logical

```text
Cursor Skill / CLI
        |
        v
Application Services
  - Source Management
  - Sync Orchestrator
  - Search Service
  - Freshness/Notification Service
        |
        v
Domain Model + Ports
  - Resource identity
  - Sync state
  - Job state
  - Query AST
        |
  +-----+-------------------+
  |                         |
  v                         v
SQLite/FTS5 Adapter     Provider Adapters
  - metadata            - GitHub REST
  - FTS5                - GitHub GraphQL
  - jobs                - Wiki Git
  - checkpoints         - rate limit/auth
```

Dependency direction:

```text
adapters -> application -> domain
infrastructure implements ports defined by application/domain
CLI/skill depends on application interfaces, never on SQLite or GitHub models
```

### 7.2. Domain layer

Domain không biết SQLite, HTTP, GraphQL, GitHub SDK hoặc CLI flags.

Các value object/state chính:

- `SourceID`;
- `ResourceID`;
- `ResourceType`;
- `ResourceSnapshot`;
- `ResourceRelation`;
- `CollectionID`;
- `Freshness`;
- `SyncHealth`;
- `SyncCheckpoint`;
- `SyncJob`;
- `SearchQuery`;
- `SearchResult`.

Các invariant phải được enforce ở domain/application:

- resource identity không rỗng;
- resource type thuộc finite set đã đăng ký;
- child resource không trỏ sang source khác;
- checkpoint chỉ advance sau persistence thành công;
- `confirmed_stale` cần evidence hoặc explicit provider signal;
- FTS result luôn map được về active local resource;
- cancellation không biến thành success.

### 7.3. Application layer

Application services đề xuất:

- `SourceService`: add/list/enable/disable/remove source;
- `SyncService`: tạo job, cancel, retry, full sync;
- `SyncOrchestrator`: lập kế hoạch collection và dependency;
- `WorkerService`: lease job, fetch page, persist batch, advance checkpoint;
- `SearchService`: parse query, execute local search, optional remote fallback;
- `FreshnessService`: tính freshness, enqueue refresh, phát event;
- `StatusService`: aggregate source, collection, job và rate-limit state;
- `MaintenanceService`: migrations, integrity check, prune và rebuild FTS.

Mỗi service nhận dependency qua interface. Không dùng global singleton cho
database, clock, token hoặc network client.

### 7.4. Provider adapters

GitHub adapter phải tách thành:

- REST client cho endpoint phù hợp với issues, PRs, comments, reviews và
  metadata;
- GraphQL client cho Projects v2 hoặc query cần connection/cursor;
- wiki adapter cho wiki git;
- capability detector;
- rate-limit và pagination abstraction;
- mapper từ provider DTO sang canonical domain snapshot.

Provider DTO không được lan vào domain hoặc persistence. Mapper phải validate
untrusted response trước khi tạo domain object.

Indexer không dùng GitHub search endpoint làm primary ingestion source. Mỗi
collection dùng endpoint chính thức phù hợp, pagination riêng, checkpoint riêng
và capability report riêng.

### 7.5. Persistence adapter

SQLite dùng:

- WAL mode;
- foreign key enforcement;
- busy timeout;
- schema migration version;
- một writer coordinator trong process;
- read connection pool giới hạn;
- transaction ngắn theo page/batch.

Network I/O và scheduler chạy async; SQLite blocking work phải được cô lập trong
writer task hoặc blocking pool phù hợp với Rust async runtime. Batch channel giữa
worker và writer phải có capacity giới hạn để tạo backpressure.

MVP có đúng một process owner cho worker queue. `daemon run` là long-running
local worker process; `sync --wait` có thể xử lý job foreground khi daemon không
chạy. Nếu CLI và daemon cùng truy cập database, phải có process lock hoặc cơ
chế lease rõ ràng để không chạy duplicate worker không kiểm soát.

## 8. Local data model

Tên bảng có thể thay đổi khi implementation, nhưng semantics sau là bắt buộc.

### `sources`

Lưu repository configuration và canonical identity:

- `id`;
- `provider`;
- `owner`;
- `repository`;
- `canonical_url`;
- `credential_profile`;
- `enabled`;
- `created_at`;
- `updated_at`.

### `resources`

Lưu canonical snapshot:

- `id`;
- `source_id`;
- `resource_type`;
- `remote_id`;
- `remote_node_id`;
- `parent_resource_id`;
- `canonical_url`;
- `title`;
- `body`;
- `state`;
- `author_login`;
- `remote_created_at`;
- `remote_updated_at`;
- `local_synced_at`;
- `remote_deleted_at`;
- `content_hash`;
- `raw_payload` hoặc reference tới raw payload;
- `created_at`;
- `updated_at`.

Unique constraint tối thiểu:

```text
(source_id, resource_type, remote_id)
```

### `resource_relations`

Lưu quan hệ không phù hợp với một parent duy nhất, ví dụ:

- PR thuộc repository;
- review thuộc PR;
- inline comment thuộc review và PR;
- project item liên kết tới issue/PR;
- wiki page liên kết tới revision.

Quan hệ phải có loại và unique constraint để upsert idempotent.

### `fts_documents`

FTS5 document có:

- local resource ID không tokenize;
- source ID không tokenize;
- resource type;
- title;
- body/content;
- searchable facets;
- parent context nếu cần ranking hoặc display.

FTS document của resource deleted phải bị loại khỏi default search hoặc xoá
trong cùng transaction. Rebuild FTS phải là command có thể chạy lại.

### `sync_collections`

Một row cho mỗi `(source, collection)`:

- `source_id`;
- `collection`;
- `capability_state`;
- `freshness`;
- `sync_health`;
- `cursor`;
- `updated_watermark`;
- `etag`;
- `last_checked_at`;
- `last_sync_started_at`;
- `last_success_at`;
- `last_failure_at`;
- `last_error_code`;
- `last_error_message_safe`;
- `consecutive_failures`;
- `next_sync_at`;
- `items_seen`;
- `items_indexed`;
- `items_deleted`;
- `schema_version`.

`last_error_message_safe` không được chứa token, Authorization header hoặc raw
private payload ngoài phần cần thiết để debug.

### `sync_jobs`

Durable queue cần các field:

- `id`;
- `source_id`;
- `collection`;
- `kind` (`initial`, `incremental`, `full_reconcile`, `rebuild`);
- `state`;
- `priority`;
- `attempts`;
- `available_at`;
- `lease_until`;
- `worker_id`;
- `checkpoint_snapshot`;
- `last_error_code`;
- `created_at`;
- `started_at`;
- `finished_at`.

### `sync_events`

Lưu event local để status/notification đọc lại:

- `id`;
- `source_id`;
- `collection`;
- `event_type`;
- `dedupe_key`;
- `payload_safe`;
- `created_at`;
- `acknowledged_at`.

### Raw payload và migration

Raw payload nên được giữ để debug và forward compatibility, nhưng canonical
normalized fields mới là contract cho search. Nếu payload vượt giới hạn cấu hình,
hệ thống phải ghi rõ `raw_payload_truncated = true` và không được truncate title
hoặc body mà không đánh dấu.

Schema phải có migration version và test upgrade từ database version trước đó.

## 9. Thiết kế sync incremental

### 9.1. Sync plan

Mỗi source có sync plan gồm các collection độc lập:

1. repository metadata và capability;
2. issues;
3. issue comments;
4. pull requests;
5. PR conversation comments;
6. PR reviews;
7. PR review comments;
8. projects;
9. project items/field values;
10. wiki.

Collection con có thể chạy sau khi parent đã có identity. Các collection độc lập
có thể chạy song song trong giới hạn concurrency.

### 9.2. Page transaction

Worker xử lý theo chu trình:

1. lease job;
2. đọc checkpoint;
3. gọi provider với cancellation token và timeout;
4. validate và map từng item;
5. mở transaction ngắn;
6. upsert resources và relations;
7. upsert/delete FTS documents;
8. ghi metrics/counts;
9. ghi checkpoint mới;
10. commit;
11. chỉ sau commit mới fetch page tiếp theo.

Nếu process chết trước commit, page được lấy lại và idempotency bảo đảm không
duplicate. Nếu chết sau commit, checkpoint đã tương ứng với dữ liệu đã persist.

### 9.3. Checkpoint strategy

Checkpoint phải mô tả đúng loại cursor:

- GraphQL connection cursor;
- REST page/high-water mark;
- ETag;
- wiki git commit;
- full reconciliation generation.

Không dùng một cursor generic không có `kind`, vì cursor của collection này có
thể không hợp lệ cho collection khác.

Với collection có `updated_at`, sử dụng overlap window configurable để giảm nguy
cơ bỏ sót item có cùng timestamp hoặc clock boundary. Dedupe dựa trên remote
identity và content hash, không dựa chỉ trên timestamp.

### 9.4. Reconciliation và deletion

Incremental sync không phải lúc nào cũng phát hiện deletion. Hệ thống phải có:

- incremental job để cập nhật nhanh;
- full reconcile job theo lịch hoặc explicit command;
- generation marker để nhận biết resource không còn xuất hiện;
- chỉ tombstone/delete sau khi full reconcile hoàn tất hợp lệ.

Nếu full reconcile bị dừng giữa chừng, không được xoá hàng loạt resource dựa
trên kết quả chưa hoàn chỉnh.

### 9.5. Rate limit và backpressure

Rate limiter phải dùng thông tin rate limit provider nếu có, tôn trọng
`Retry-After`, và dừng enqueue thêm request khi budget không đủ.

Scheduler phải ưu tiên:

1. job user yêu cầu explicit;
2. retry đến hạn;
3. collection đã stale;
4. full reconcile định kỳ.

Mỗi source cần có fairness để một source lớn không starve source khác.

## 10. Search design

### 10.1. Query pipeline

```text
raw query
  -> lexer/parser
  -> validated SearchQuery
  -> SQL filters + FTS5 match
  -> relation/parent enrichment
  -> freshness/status enrichment
  -> stable result ordering
```

Query parser phải từ chối qualifier không hợp lệ với lỗi có cấu trúc. Không
chuyển nguyên văn input của người dùng vào SQL hoặc FTS expression mà không
validate/escape.

### 10.2. Ranking và result grouping

MVP search từng document độc lập để không làm mất comment hoặc inline comment.
Có thể hỗ trợ `group_by=parent` sau đó để gom nhiều hit dưới issue/PR.

Ordering mặc định:

1. FTS5 BM25 relevance;
2. remote updated time;
3. local resource ID ổn định.

Không dùng local insertion time làm ranking chính vì sync lại có thể thay đổi
thứ tự không có ý nghĩa.

### 10.3. Local result semantics

Local search phải trả được dù:

- network down;
- token hết hạn;
- một collection đang retry;
- một resource đã confirmed stale.

Result phải hiển thị stale badge và đường dẫn sync/status để người dùng tự đánh
giá độ tin cậy thay vì âm thầm trả dữ liệu cũ.

## 11. CLI và Cursor Skill contract

### 11.1. CLI tối thiểu

Tên binary đề xuất: `github-local-indexer`.

Các command P0:

```text
repo add <owner>/<repo>
repo list
repo enable <source>
repo disable <source>
daemon run [--foreground]
sync [<source>] [--wait] [--full]
jobs list
jobs cancel <job-id>
search <query> [--source ...] [--type ...] [--json]
status [<source>] [--json]
resource show <resource-id>
doctor
index rebuild
```

Quy ước:

- command không cần network phải hoạt động khi offline;
- output human-readable mặc định, JSON schema ổn định với `--json`;
- exit code phân biệt invalid input, partial failure, auth/permission,
  transient failure và cancellation;
- `sync` mặc định enqueue và trả về job ID;
- `sync --wait` chạy tới khi job kết thúc hoặc bị cancel;
- không log token, request header hoặc full private raw payload.

### 11.2. Cursor Skill

Skill phải:

- nhận intent index/search/status;
- gọi binary qua interface ổn định;
- dùng local search mặc định;
- hỏi xác nhận trước khi add source hoặc chạy remote fallback nếu action đó
  không được user yêu cầu rõ;
- truyền `--json` để parse kết quả;
- hiển thị freshness và sync failure cùng search result;
- không tự triển khai GitHub API logic;
- không tự lưu credential.

Skill không được biến một câu hỏi search local thành một request GitHub ngầm.

### 11.3. Local API

HTTP API loopback không bắt buộc trong MVP. Nếu consumer ngoài CLI thực sự cần,
chỉ bind `127.0.0.1` mặc định, dùng schema tương tự JSON CLI và không expose
credential hoặc raw token.

## 12. Authentication và security

### 12.1. Credential

MVP phải hỗ trợ token từ environment cho automation, ví dụ `GITHUB_TOKEN`.
Interactive mode nên hỗ trợ OS keychain; token không được lưu trong SQLite
hoặc command history.

Credential profile phải tách khỏi source metadata để một token có thể dùng cho
nhiều source mà không duplicate secret.

Wiki git fetch không được nhúng token trực tiếp vào persistent remote URL.

### 12.2. Least privilege

Provider adapter phải kiểm tra capability/permission theo endpoint. Khi token
thiếu quyền, lỗi cần nói collection nào bị ảnh hưởng và cách khắc phục, không
in token hoặc toàn bộ request.

### 12.3. Local data protection

- database file mặc định chỉ user hiện tại đọc/ghi được;
- không bind network interface ngoài loopback;
- log redact Authorization, token, cookie và query có secret;
- raw Markdown/HTML không được execute trong CLI;
- UI consumer phải sanitize content trước khi render;
- path, URL và owner/name phải được validate chống path traversal nếu dùng để
  lưu wiki hoặc artifact;
- database có thể chứa private data nên phải mô tả retention và purge command.

## 13. Non-functional requirements

### NFR-001 — Correctness

Không mất resource đã commit khi process restart. Mọi record phải truy ngược
được về source và remote identity.

### NFR-002 — Crash safety

Crash tại bất kỳ điểm nào giữa hai page phải resume được từ checkpoint hợp lệ.
Không đánh dấu job thành công nếu transaction cuối chưa commit.

### NFR-003 — Local search latency

Mục tiêu benchmark MVP trên máy developer với khoảng 100.000 documents:

- p95 local search trả page đầu không quá 200 ms;
- không phát sinh network request;
- memory search tăng theo page size, không theo toàn bộ database.

Đây là target cần benchmark, không phải lý do để hy sinh correctness.

### NFR-004 — Bounded resource use

Worker, queue, page size, retry delay, request timeout và database connection
pool đều phải cấu hình được với default an toàn. Không load toàn bộ repository
hoặc toàn bộ raw payload vào memory.

### NFR-005 — Portability

MVP nên chạy được trên macOS và Linux, có data directory override và không phụ
thuộc daemon hệ điều hành cụ thể. Các OS notification backend là optional.

### NFR-006 — Observability

Structured local logs và `status --json` phải cho biết:

- source/collection;
- job ID;
- items fetched/indexed/deleted;
- current checkpoint kind;
- request/retry count;
- rate-limit state nếu provider trả về;
- duration;
- sanitized error code.

Không gửi telemetry ra ngoài mặc định.

### NFR-007 — Maintainability

Thêm resource type mới phải chỉ tác động adapter, mapper, schema registration,
sync strategy, search field mapping và tests liên quan; không sửa logic chung
của tất cả resource.

## 14. Error model

Error phải là typed error có category và retryability:

- `invalid_input`;
- `authentication_failed`;
- `permission_denied`;
- `not_found`;
- `rate_limited`;
- `provider_transient`;
- `provider_payload_invalid`;
- `local_storage`;
- `migration_failed`;
- `cancelled`;
- `unsupported`.

Application layer quyết định retry dựa trên category. CLI quyết định exit code
dựa trên category. Raw provider error chỉ được lưu sau khi redact và giới hạn
kích thước.

## 15. Testing strategy

### Unit tests

Phải có test cho:

- source và resource identity;
- normalization và validation provider payload;
- state transition của freshness/sync health;
- cursor/checkpoint;
- idempotent upsert;
- deletion và full reconciliation generation;
- query parser và filter;
- retry/backoff classification;
- notification dedupe;
- cancellation không advance checkpoint.

### Integration tests

SQLite integration test phải kiểm tra:

- foreign key và unique constraint;
- transaction resource + relation + FTS;
- crash/retry simulation;
- reclaim expired job lease;
- FTS rebuild;
- schema migration.

Provider contract tests dùng fixture/fake server để kiểm tra:

- pagination;
- GraphQL cursor;
- ETag/304;
- updated watermark overlap;
- 429/Retry-After;
- 401/403/404/5xx;
- malformed payload;
- permission thiếu cho Projects/Wiki;
- edited/deleted comment;
- duplicate page.

### End-to-end tests

Một fixture repository phải cover:

- issue có nhiều comment;
- PR có conversation comment, review và inline comment;
- project có nhiều field/value;
- wiki page có revision;
- resource được edit, delete và reappear trong reconciliation;
- offline search trong lúc remote bị tắt;
- `sync` enqueue, worker consume, `status` và `search --json`.

Phải chạy `cargo test`, `cargo fmt --check` và `cargo clippy -- -D warnings`.
Concurrency paths phải có test cancellation, lease recovery và bounded worker.
Nếu dùng `unsafe`, phải có invariant được ghi nhận và chạy thêm Miri hoặc
sanitizer phù hợp; mặc định implementation nên tránh `unsafe`.

## 16. Delivery plan

### Phase 0 — Contract và foundation

- chốt decision gates;
- tạo domain model và capability contract;
- schema migration đầu tiên;
- CLI skeleton;
- fake provider và test fixtures;
- local search contract.

### Phase 1 — P0 ingestion

- repository metadata;
- issues và issue comments;
- pull requests, comments, reviews, inline review comments;
- SQLite/FTS5 upsert;
- initial/incremental sync;
- job queue và bounded worker.

### Phase 2 — Freshness và operations

- freshness policy;
- stale/failed/recovered events;
- `status`, `jobs`, `doctor`;
- ETag/watermark/cursor;
- full reconcile và tombstone;
- retry/rate-limit/cancellation.

### Phase 3 — Projects và Wiki

- Projects v2 adapter;
- wiki git adapter;
- capability/permission report;
- project field/value và wiki content search;
- migration/rebuild tests.

### Phase 4 — Cursor Skill

- skill wrapper;
- local-first search intent;
- index/status intent;
- JSON parsing;
- no-hidden-network test;
- UX cho stale result và partial failure.

### Phase 5 — P1 nếu có nhu cầu

- discussions, releases, reactions, timeline, project views, wiki history,
  remote fallback và OS notifications.

## 17. Acceptance criteria cho MVP

MVP chỉ được xem là đạt khi tất cả điều kiện sau đúng:

1. Thêm được ít nhất một public hoặc private repository mà token không xuất
   hiện trong database/log.
2. Initial sync được enqueue async, có job ID, daemon hoặc `--wait` có progress,
   và resume sau process restart.
3. Index được issue/comment, PR/conversation comment/review/inline comment,
   project core và wiki current content theo capability/permission thực tế.
4. Search local tìm được body/title/comment/review/inline comment/wiki và trả
   parent context, URL, type, remote/local timestamps.
5. `search` khi network down vẫn trả local result và không tạo network request.
6. Sửa resource trên fake GitHub rồi chạy incremental sync sẽ update đúng một
   local record và FTS document, không duplicate.
7. Xoá resource chỉ bị tombstone sau full reconciliation hoàn tất; partial
   reconcile không xoá hàng loạt.
8. Lỗi collection con không xoá hoặc làm mất collection đã thành công.
9. TTL hết hạn tạo `possibly_stale`; remote version mới tạo
   `confirmed_stale`; refresh thất bại giữ được local data và báo `failed`.
10. `status --json` phân biệt được `remote_updated_at`,
    `local_synced_at`, `last_checked_at`, freshness và sync health.
11. Retry bounded, tôn trọng rate limit, cancellation và expired lease.
12. FTS search p95 đạt target benchmark đã nêu trên dataset test.
13. Schema migration, FTS rebuild, provider fixtures, async concurrency tests,
    `cargo fmt --check` và `cargo clippy -- -D warnings` đều có trong CI của
    project mới.

## 18. Decision gates cần xác nhận trước implementation

Các điểm sau cần chốt rõ để tránh thay đổi infrastructure hoặc contract giữa
chừng:

1. **Runtime**: Rust CLI/daemon + SQLite/FTS5 với BM25 là baseline đã chọn;
   implementation cần chốt async runtime và dependency policy.
2. **Storage**: SQLite + FTS5 là local source of truth cho MVP.
3. **Execution model**: một local daemon làm worker process owner; `sync --wait`
   là fallback foreground, không cho nhiều worker process cùng database.
4. **Freshness defaults**: 15 phút cho issue/PR/comment/review, 60 phút cho
   repository/project/wiki và 24 giờ cho full reconciliation; có thể điều chỉnh
   sau benchmark.
5. **Credential**: environment cho automation và OS keychain cho interactive;
   không lưu token trong SQLite.
6. **Wiki**: wiki git fetch là nguồn chính; P0 chỉ index current revision,
   history để P1.
7. **Remote fallback**: để P1; nếu triển khai phải explicit và không làm thay
   đổi semantics local-first.
8. **Skill interface**: CLI JSON là contract duy nhất ở P0; loopback HTTP để P1.

Cho tới khi các gate này được xác nhận, không nên thêm Redis, Elasticsearch,
cloud service, public webhook endpoint hoặc write-back integration.
