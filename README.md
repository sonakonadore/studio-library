# 스튜디오 라이브러리

Apple Silicon Mac용 오프라인 프로젝트·샘플 정리 앱. 메뉴와 버튼은 영어로, 사용 안내는 한국어로 표시됩니다. 화면은 Rust의 eframe/egui, 카탈로그는 SQLite, 미리듣기는 Core Audio를 사용합니다. 설정, 사용자 태그, 가상 폴더는 `~/Library/Application Support/Studio Library/library.sqlite3`에 저장됩니다.

## 프로젝트 정리

1. **Choose folder**으로 프로젝트 폴더나 외장 드라이브를 지정합니다. 탐색은 선택한 위치에서 시작합니다.
2. **Discovered projects**에서 곡과 버전을 확인합니다. 파일·폴더의 `v02` 같은 표기를 인식합니다. 같은 제목이 여러 위치에 있거나 버전이 겹치면 **Needs review**에서 곡 이름, 버전, 묶음을 저장할 수 있습니다.
3. **Plan a copy**에서 출발지, 대상, 예상 용량을 확인하고 승인합니다. 대상은 `곡 묶음/버전/프로젝트`입니다. 파일형 세션과 지원되는 미디어 폴더의 상대 경로, 패키지형 세션의 내부 구조를 보존합니다.
4. 근처 오디오 후보나 직접 추가한 관련 파일은 **참조 미확인**으로 표시됩니다. 이미 프로젝트 구조에 포함된 파일은 한 번만 복사합니다. 범위 밖에서 직접 선택한 관련 파일은 복사본의 **관련 파일**에 둡니다.

**자동 탐지 형식**

| DAW | 형식 |
| --- | --- |
| Ableton Live | `.als` |
| Logic Pro | `.logicx` |
| GarageBand | `.band` |
| Fender Studio Pro | `.song` |
| Cubase | `.cpr` |
| FL Studio | `.flp` |
| REAPER | `.rpp` |
| Pro Tools | `.ptx` |
| Bitwig Studio | `.bwproject` |

**Add file**이나 **Add folder**으로 다른 형식의 개별 프로젝트를 지정합니다. Finder에서 파일처럼 보이는 패키지는 **Add file**에서 고를 수 있습니다. 반복해서 찾으려면 **DAW and detection rules**에서 기본 형식을 선택하거나 DAW 이름과 확장자 또는 정확한 폴더 이름을 입력하고 저장합니다. 확장자와 폴더 규칙은 대소문자 구분 없이 적용됩니다.

외부 미디어를 모을 때는 DAW에서 먼저 수집합니다. Ableton Live는 **File → Collect All and Save**, Logic Pro는 **File → Project Management → Consolidate**를 사용합니다. [Ableton Live 매뉴얼](https://www.ableton.com/en/live-manual/12/managing-files-and-sets/) · [Apple Logic Pro 가이드](https://support.apple.com/en-euro/guide/logicpro/lgcpce09b9d8/12.3/mac/15.6)

Fender Studio Pro는 **File → Save to New Folder**를 사용합니다. `.song`의 상위 폴더를 탐색 위치에 추가하면 같은 위치의 `Media`와 `History`가 복사됩니다. `History`는 탐색에서 제외됩니다. [Save to New Folder 공식 안내](https://support.presonus.com/hc/en-us/articles/29973594324749-Studio-One-Pro-7-Exploring-the-Save-Options)

## 샘플 라이브러리

1. 다운로드한 샘플이 들어 있는 로컬 Splice 폴더를 선택합니다. **Scan for changes**은 크기·수정 시각·파일 식별자 등이 달라진 파일만 다시 디코딩합니다.
2. 파일명 검색과 **Filter by tag**를 함께 쓸 수 있습니다. 여러 태그를 고르면 모든 태그가 일치하는 샘플을 표시합니다. **Save filters as folder**는 해당 태그 조합을 저장합니다.
3. 샘플 이름을 클릭해 미리듣기, 정지, 볼륨 조절, Finder에서 보기를 사용합니다. 태그를 편집하면 재시작과 재분석 후에도 사용자 태그가 유지됩니다.
4. 체크박스로 필요한 샘플만 선택해 **Copy selected**로 개인 폴더에 복사합니다. 복사 대상이 같은 이름을 이미 사용하고 있으면 충돌을 표시합니다.

킥, 스네어, 클랩, 림샷, 클로즈드 햇, 오픈 햇, 셰이커 등은 개별 태그로 표시합니다. 장르는 명시적인 메타데이터나 파일명·폴더명의 확실한 장르 표기에서 제안합니다. 검색 결과 오른쪽에서 오디오의 재생 길이를 확인할 수 있습니다. 기존 항목의 제안 태그는 **Scan for changes**으로 갱신할 수 있습니다. 길이는 오디오 정보에서 읽고, 밝기는 신호 변화량을, 형태는 이름과 에너지의 반복 또는 감쇠를 이용해 추정합니다. 판단이 애매하면 **Unclassified**로 표시합니다. WAV, AIFF, MP3를 지원합니다. 탐색과 복사는 백그라운드에서 실행되며 심볼릭 링크는 건너뜁니다.

## 빌드와 검증

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
./scripts/package-macos.sh
```

패키징 결과는 `dist/Studio Library.app`입니다. 개인 Mac용 ad hoc 서명을 적용합니다. Launchpad·Finder·Dock 아이콘은 `assets/StudioLibrary.icns`에서 지정합니다. 공증 상태는 미공증입니다.

- Apple M1 Pro, macOS 14.7.3에서 앱을 열고 테스트 후보 탐색, Fender 복사 승인, 샘플 검색·AIFF 미리듣기·정지, 선택 복사를 확인했습니다.
- Logic Pro 10.7.5에서 합성 WAV를 담은 테스트 패키지의 복사본을 열었습니다. 테스트 원본 경로를 비운 상태에서도 오디오 출력 미터를 확인했습니다.
- Logic Pro 외의 DAW는 **세션 열기·미디어 미검증**입니다. Fender는 가짜 `.song`의 `Media`·`History` 보존까지 확인했습니다.
- 자동 테스트는 동명 곡, 버전, 패키지, 직접 등록, 탐색 규칙, WAV·AIFF·MP3 카탈로그, 태그 유지, 중복 방지, 취소, 읽기 권한 오류, 탐색 루트 교체를 포함합니다. 외장 드라이브 분리는 루트 폴더를 교체해 모의 시험했습니다.
