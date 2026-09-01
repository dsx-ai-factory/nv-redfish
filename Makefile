#
# Build and test everything.
#

pwd := $(shell pwd)

maybe-lenovo-check = $(if $(wildcard $(pwd)/oem/lenovo/*.xml),cargo check --features oem-lenovo)

space := $(empty) $(empty)
comma :=,
indent := $(empty)	$(empty)
define new-line
$(empty)
$(empty)
endef

# We cannot use --all-features because they depends on oem files that
# are not distributed by the repo.
all-std-features = accounts \
                   assembly \
                   bios \
                   boot-options \
                   chassis \
                   computer-systems \
                   ethernet-interfaces \
                   log-services \
                   managers \
                   manager-network-protocol \
                   memory \
                   network-adapters \
                   ports \
                   power \
                   power-equipment \
                   power-supplies \
                   processors \
                   secure-boot \
                   sensors \
                   session-service \
                   storages \
                   thermal \
                   update-service \
                   event-service \
                   task-service

# Features that cannot be compiled standalone (no references from the tree).
std-not-standalone-features = assembly \
             bios \
             boot-options \
             ethernet-interfaces \
             log-services \
             manager-network-protocol \
             network-adapters \
             ports \
             processors \
             power \
             power-supplies \
             secure-boot \
             sensors \
             storages \
             update-service-deprecated

std-standalone-features = $(filter-out $(std-not-standalone-features),$(all-std-features))

ci-features-list := $(subst $(space),$(comma),$(all-std-features)),http-extras,resource-serialization

# Feature sets whose only job is to prove the configuration type checks.
# Nothing downstream consumes the artifacts, so they run under `cargo check`:
# it stops after type checking and skips codegen, LLVM and linking, which is
# where nearly all the time goes for a crate built from tens of thousands of
# generated lines. Codegen and linking still get covered for the whole
# workspace by the `cargo build`/`cargo test` steps below.
compile-only-feature-sets = computer-systems,processors,controls \
             managers,oem-hpe \
             managers,oem-supermicro \
             chassis,power-supplies,oem-liteon \
             chassis,controls \
             chassis,network-adapters \
             chassis,network-adapters,network-device-functions \
             update-service-deprecated \
             bmc-http,update-service-deprecated \
             http-extras \
             computer-systems,bios,boot-options,storages,memory,processors \
             oem-hpe,accounts \
             oem-hpe \
             oem-nvidia \
             computer-systems,oem-nvidia \
             chassis,oem-nvidia \
             computer-systems,processors,memory,sensors,telemetry-service,oem-nvidia \
             telemetry-service \
             environment-metrics,memory,oem-nvidia \
             oem-dell \
             oem-ami \
             managers,oem-dell-attributes \
             $(std-standalone-features) \
             ""

check-one-feature = $(indent)cargo check -p nv-redfish --features $1$(new-line)

define build-and-test
	cargo fmt --all -- --check
	cargo clippy $1
	cargo clippy -p nv-redfish-dispatcher --all-targets
	cargo clippy -p nv-redfish-dispatcher --all-targets --all-features
	cargo clippy -p nv-redfish-bmc-http --bench cache
	$(foreach f,$(compile-only-feature-sets),$(call check-one-feature,$f))
	$(maybe-lenovo-check)
	cargo check -p nv-redfish
	cargo check -p nv-redfish-bmc-http --no-default-features --features http-extras
	cargo check -p nv-redfish-tests --tests
	cargo check -p nv-redfish-bmc-mock
	cargo build -p update-multipart --features update-service-deprecated
	cargo build
	cargo build $1
	cargo test $1 -- --no-capture
	cargo test -p nv-redfish-bmc-http --test reqwest_client_tests --features reqwest,update-service-deprecated
	cargo test -p nv-redfish-tests --test test-update-service --features update-service-deprecated
	cargo test -p nv-redfish-dispatcher --all-features -- --no-capture
	cargo doc --no-deps $1

endef


all:
	$(call build-and-test,--all-features)

ci: rust-install
	$(call build-and-test,--features $(ci-features-list))

rust-install:
	rustup component add clippy rustfmt

clean:
	rm -rf $(schema-dir)
	rm -rf target
