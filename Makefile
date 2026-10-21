#
# Build and test everything.
#

pwd := $(shell pwd)

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
                   certificates \
                   chassis \
                   component-integrity \
                   computer-systems \
                   ethernet-interfaces \
                   fabrics \
                   job-service \
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
                   serial-interfaces \
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
             serial-interfaces \
             storages \
             update-service-deprecated

std-standalone-features = $(filter-out $(std-not-standalone-features),$(all-std-features))

ci-features-list := $(subst $(space),$(comma),$(all-std-features)),bmc-http,http-extras,resource-serialization

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
             bmc-http,nv-redfish-bmc-http/update-service-deprecated \
             http-extras \
             computer-systems,bios,boot-options,storages,memory,processors \
             oem-hpe,accounts \
             oem-hpe \
             oem-nvidia \
             fabrics,oem-nvidia \
             fabrics,ports,oem-nvidia \
             chassis,fabrics \
             managers,fabrics \
             std-redfish,oem-nvidia \
             oem-nvidia-fabrics \
             fabrics,oem-nvidia-fabrics \
             computer-systems,oem-nvidia \
             managers,oem-nvidia \
             chassis,oem-nvidia \
             chassis,network-adapters,oem-nvidia \
             computer-systems,processors,memory,sensors,telemetry-service,oem-nvidia \
             telemetry-service \
             environment-metrics,memory,oem-nvidia \
             oem-dell \
             oem-ami \
             managers,oem-ami \
             update-service,oem-ami \
             oem-lenovo \
             accounts,oem-lenovo \
             computer-systems,oem-lenovo \
             managers,oem-lenovo \
             chassis,network-adapters,ports,oem-lenovo \
             accounts,oem-dell \
             accounts,managers,oem-dell \
             managers,oem-dell \
             managers,oem-dell-attributes \
             computer-systems,storages,oem-dell \
             job-service,oem-dell \
             managers,job-service,oem-dell \
             managers,job-service,oem-dell,oem-dell-attributes \
             $(std-standalone-features) \
             ""

check-one-feature = $(indent)cargo check -p nv-redfish --features $1$(new-line)

# Builds of the reqwest client without ring, as package:features. Each must
# build, and ring must not be in its dependency tree on any target, so one
# platform checks them all.
no-ring-builds = nv-redfish-bmc-http:reqwest-client \
             nv-redfish-bmc-http:reqwest-native-tls \
             nv-redfish-bmc-http:reqwest-rustls-no-provider \
             nv-redfish:bmc-http-native-tls \
             nv-redfish:bmc-http-rustls-no-provider \
             nv-redfish:bmc-http-native-tls,http-extras \
             nv-redfish:bmc-http-rustls-no-provider,http-extras

no-ring-args = -p $(word 1,$(subst :, ,$1)) --no-default-features --features $(word 2,$(subst :, ,$1))
check-no-ring = $(indent)cargo check $(call no-ring-args,$1)$(new-line)$(indent)tree=$$(cargo tree $(call no-ring-args,$1) -e no-dev --prefix none --target all) && ! printf '%s\n' "$$tree" | grep '^ring '$(new-line)

# CI runs the three groups below as separate jobs. Only `test` runs on every
# OS; `lint` and `features` are platform independent and run on Linux alone.
define lint
	cargo fmt --all -- --check
	cargo clippy $1
	cargo clippy -p nv-redfish-dispatcher --all-targets
	cargo clippy -p nv-redfish-dispatcher --all-targets --all-features
	cargo clippy -p nv-redfish-quirks --all-targets
	cargo clippy -p nv-redfish-bmc-http --bench cache
	cargo doc --no-deps $1

endef

define features
	$(foreach f,$(compile-only-feature-sets),$(call check-one-feature,$f))
	cargo check
	cargo check -p nv-redfish
	cargo check -p nv-redfish-bmc-mock
	cargo check -p nv-redfish-bmc-http --no-default-features --features http-extras
	$(foreach b,$(no-ring-builds),$(call check-no-ring,$b))
	cargo build -p update-multipart --features update-service-deprecated

endef

# `cargo test $1` builds every target the workspace has, so no separate
# `cargo build` precedes it. TEST_ARGS reaches the test harness, e.g.
# `--skip compile_fails` to leave the trybuild tests to one platform.
define test
	cargo test $1 -- --no-capture $(TEST_ARGS)
	cargo test -p nv-redfish-bmc-http --no-default-features --features reqwest-native-tls --test reqwest_client_tests
	cargo test -p nv-redfish-bmc-http --no-default-features --features reqwest-rustls-no-provider --test reqwest_client_tests
	cargo test -p nv-redfish-bmc-http --test reqwest_client_tests --features reqwest,update-service-deprecated
	cargo test -p nv-redfish-tests --test integration --features update-service-deprecated -- update_service::
	cargo test -p nv-redfish-dispatcher --all-features -- --no-capture

endef

ci-features-args = --features $(ci-features-list)

all:
	$(call lint,--all-features)
	$(call features)
	$(call test,--all-features)

ci: ci-lint ci-features ci-test

ci-lint: rust-install
	$(call lint,$(ci-features-args))

ci-features:
	$(call features)

ci-test:
	$(call test,$(ci-features-args))

rust-install:
	rustup component add clippy rustfmt

clean:
	rm -rf $(schema-dir)
	rm -rf target
