use kube::Client;

pub async fn get_client() -> Result<Client, kube::Error> {
    Client::try_default().await
}
