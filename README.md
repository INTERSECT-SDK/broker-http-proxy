# Helm Charts

This github pages website is used to host Helm charts associated with the [main repo](https://github.com/intersect-sdk/broker-http-proxy/).

## Usage

[Helm](https://helm.sh) must be installed to use the charts.  Please refer to
Helm's [documentation](https://helm.sh/docs) to get started.

Once Helm has been set up correctly, add the repo as follows:

  `helm repo add <alias> https://intersect-sdk.github.io/broker-http-proxy`

If you had already added this repo earlier, run `helm repo update` to retrieve
the latest versions of the packages.  You can then run `helm search repo
<alias>` to see the charts.

Available charts:
- `broker-2-http`
- `http-2-broker`

To install a chart:

    helm install <chart-name> <alias>/<chart-name>

To uninstall the chart:

    helm delete <chart-name>

If using an umbrella chart, add these lines to your `dependencies` section (change `version` accordingly):

```yaml
  - name: broker-2-http
    repository: https://intersect-sdk.github.io/broker-http-proxy/
    version: "0.1.0"
  - name: http-2-broker
    repository: https://intersect-sdk.github.io/broker-http-proxy/
    version: "0.1.0"
```
