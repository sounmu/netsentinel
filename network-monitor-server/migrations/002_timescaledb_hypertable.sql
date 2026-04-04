-- ============================================================
-- Migration: TimescaleDB Hypertable 변환 + 자동 보존 정책
-- ============================================================
-- 이 파일은 init_db()에서 자동 실행됩니다.
-- 수동 실행: psql -d network_monitor -f 002_timescaledb_hypertable.sql

-- 1) TimescaleDB 확장 활성화
--    timescale/timescaledb Docker 이미지에 이미 포함되어 있으므로
--    CREATE EXTENSION만으로 활성화 가능
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 2) Hypertable 변환 (1일 단위 청크 파티셔닝)
--
--    [파티셔닝 이점]
--    - 시간 범위 쿼리 시 무관한 청크를 자동으로 건너뛰어 I/O 대폭 절감
--    - 10초 수집 주기 기준 일별 ~8,640행/호스트 → 1일 청크가 적절한 크기
--    - 인덱스도 청크 단위로 분리되어 B-Tree 깊이가 얕게 유지됨
--
--    [옵션 설명]
--    - if_not_exists: 이미 hypertable이면 무시 (재실행 안전)
--    - migrate_data:  기존 행을 시간 기준 청크로 재배치
SELECT create_hypertable(
    'metrics',
    'timestamp',
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE,
    migrate_data => TRUE
);

-- 3) 자동 보존 정책: 90일 초과 청크를 DROP
--
--    [자동 삭제 성능 이점]
--    - 기존 방식: 행 단위 DELETE → 대량 dead tuple → VACUUM 부하 + 테이블 Bloat
--    - TimescaleDB: 청크(파티션) 단위 DROP → O(1) 메타데이터 연산, Bloat 제로
--    - 백그라운드 워커가 자동 실행하므로 애플리케이션 스케줄러 불필요
--
--    if_not_exists: 정책이 이미 등록되어 있으면 무시 (재실행 안전)
SELECT add_retention_policy(
    'metrics',
    INTERVAL '90 days',
    if_not_exists => TRUE
);
