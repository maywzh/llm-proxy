export K8S_CLUSTER=cn-pvg06-eng-r1-general
export NS=llmops-dev

kubectl config use-context $K8S_CLUSTER
kubectl -n $NS apply -f k8s.yaml